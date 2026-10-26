use crate::time::now_rfc3339;

use crate::error::AppError;
use crate::middleware::{dn_matches, sanitize_for_log};
use crate::models::{
    DeviceRecord, DeviceResponse, DeviceStyle, HelloResponse, RegisterDeviceRequest, UplinkMessage,
    UplinkRecord, UplinkResponse,
};
use crate::store::DeviceStore;
use crate::validate;
use supervictor_common::{fields, status};

/// Return a greeting, optionally echoing the mTLS client subject.
pub fn handle_hello(client_subject: Option<String>) -> HelloResponse {
    HelloResponse {
        message: "Hello from Supervictor!".into(),
        client_subject,
    }
}

/// Decide whether an uplink claiming `device_id` may proceed, given what is
/// on record for that device and the mTLS subject the proxy forwarded.
///
/// Independent of `require_registration`: a device registered with a
/// `subject_dn` is *bound* to that certificate, and an uplink for it must
/// present a matching subject — no certificate (e.g. a path that bypasses
/// mTLS) or a different device's certificate is rejected. This is what stops
/// one valid device cert from spoofing another device's telemetry.
///
/// With `require_registration`: the device must also exist, be `active`,
/// and be bound — a registered device with no `subject_dn` on record is
/// rejected (owner call 2026-10-23: strict mode means every device is
/// re-registered with its certificate first). Without the flag such a
/// device is still accepted, with a warn log, so today's fleet keeps
/// reporting until the flag is flipped.
pub fn authorize_uplink(
    device_id: &str,
    device: Option<&DeviceRecord>,
    client_subject: Option<&str>,
    require_registration: bool,
) -> Result<(), AppError> {
    let Some(device) = device else {
        return if require_registration {
            Err(AppError::DeviceNotRegistered)
        } else {
            Ok(())
        };
    };

    if require_registration && device.status != status::ACTIVE {
        return Err(AppError::DeviceNotRegistered);
    }

    match device.subject_dn.as_deref() {
        Some(bound) => match client_subject {
            Some(presented) if dn_matches(bound, presented) => Ok(()),
            presented => {
                tracing::warn!(
                    device_id = %sanitize_for_log(device_id),
                    presented = %sanitize_for_log(presented.unwrap_or("<none>")),
                    "uplink rejected: client certificate does not match registered subject"
                );
                Err(AppError::SubjectMismatch {
                    device_id: device_id.to_string(),
                })
            }
        },
        None if require_registration => {
            tracing::warn!(
                device_id = %sanitize_for_log(device_id),
                "uplink rejected: registered device has no bound subject_dn (REQUIRE_REGISTRATION)"
            );
            Err(AppError::DeviceNotRegistered)
        }
        None => {
            tracing::warn!(
                device_id = %sanitize_for_log(device_id),
                "uplink from registered device with no bound subject_dn (accepted; unbound)"
            );
            Ok(())
        }
    }
}

/// What an accepted uplink produced: the device-facing response and the
/// record as persisted (or as it would be, when no store is attached) —
/// the dashboard's live stream is built from the record, not re-parsed.
#[derive(Debug)]
pub struct Ingested {
    /// Response body for the device.
    pub response: UplinkResponse,
    /// The stored row: device id, receive time, payload minus `id`.
    pub record: UplinkRecord,
}

/// Parse and persist a device uplink message.
///
/// The payload `id` must pass [`validate::check_device_id`] and a `levels`
/// burst, if present, [`validate::check_levels`]. When a store is present
/// the device record is looked up and [`authorize_uplink`] decides whether
/// the request may proceed; `require_registration` additionally demands an
/// `active` registration.
pub fn handle_uplink(
    raw_body: Option<&str>,
    client_subject: Option<String>,
    store: Option<&dyn DeviceStore>,
    require_registration: bool,
) -> Result<UplinkResponse, AppError> {
    ingest_uplink(raw_body, client_subject, store, require_registration).map(|i| i.response)
}

/// [`handle_uplink`], keeping the persisted record for the caller.
pub fn ingest_uplink(
    raw_body: Option<&str>,
    client_subject: Option<String>,
    store: Option<&dyn DeviceStore>,
    require_registration: bool,
) -> Result<Ingested, AppError> {
    let body = raw_body
        .filter(|b| !b.trim().is_empty())
        .ok_or(AppError::MissingBody)?;

    let uplink: UplinkMessage =
        serde_json::from_str(body).map_err(|e| AppError::InvalidPayload {
            detail: e.to_string(),
            structured: None,
        })?;

    validate::check_device_id(fields::UPLINK_ID, &uplink.id)?;
    validate::check_levels(&uplink.levels, uplink.levels_ms)?;

    let mut desired_fw = None;
    if let Some(s) = store {
        let device = s.get_device(&uplink.id)?;
        authorize_uplink(
            &uplink.id,
            device.as_ref(),
            client_subject.as_deref(),
            require_registration,
        )?;
        desired_fw = device.and_then(|d| d.desired_fw);
    }

    // Persist every field the device sent (health metadata, sensor
    // readings, the levels burst — absent Options are already off the
    // wire), minus the id, which is the record key.
    let mut payload = serde_json::to_value(&uplink).map_err(|e| AppError::InvalidPayload {
        detail: e.to_string(),
        structured: None,
    })?;
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("id");
    }
    let record = UplinkRecord {
        device_id: uplink.id.clone(),
        received_at: now_rfc3339(),
        payload,
    };
    if let Some(s) = store {
        s.put_uplink(record.clone())?;
    }

    tracing::info!(device_id = %uplink.id, current = uplink.current, "uplink received");

    Ok(Ingested {
        response: UplinkResponse {
            message: "Uplink received".into(),
            device_id: uplink.id,
            current: uplink.current,
            client_subject,
            desired_fw,
        },
        record,
    })
}

/// Set or clear the firmware version a device should run next from a JSON
/// body (`{"desired_fw": "0.2.0"}`; `{}`, `null` or an empty body clears).
/// Setting requires that image to be stored for the device, so a typo
/// cannot point a fleet at nothing; clearing never needs the blob store.
pub fn handle_set_desired_fw(
    device_id: &str,
    raw_body: Option<&str>,
    store: &dyn DeviceStore,
    firmware: Option<&dyn crate::firmware::FirmwareStore>,
) -> Result<DeviceResponse, AppError> {
    validate::check_device_id(fields::DEVICE_ID, device_id)?;
    let invalid = |detail: String| AppError::InvalidPayload {
        detail,
        structured: None,
    };
    let req: supervictor_common::models::DesiredFwRequest =
        match raw_body.map(str::trim).filter(|b| !b.is_empty()) {
            None => Default::default(),
            Some(body) => {
                let value: serde_json::Value =
                    serde_json::from_str(body).map_err(|e| invalid(e.to_string()))?;
                match value {
                    serde_json::Value::Null => Default::default(),
                    serde_json::Value::Object(_) => {
                        serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?
                    }
                    _ => return Err(invalid("body must be a JSON object".into())),
                }
            }
        };
    if let Some(version) = req.desired_fw.as_deref() {
        crate::firmware::check_identifiers(device_id, version)?;
        let firmware = firmware.ok_or(AppError::FirmwareUnavailable)?;
        if firmware.manifest(device_id, version)?.is_none() {
            return Err(AppError::FirmwareNotFound {
                device_id: device_id.to_string(),
                version: version.to_string(),
            });
        }
    }
    let saved = store.set_desired_fw(device_id, req.desired_fw.as_deref())?;
    Ok(DeviceResponse::from(saved))
}

/// Whether `subject` may read `device`'s firmware: an admin, or the
/// device itself proving possession of the certificate it is bound to.
/// An unbound device (no `subject_dn` on record) cannot be matched, so
/// nobody but an admin reads its image — it bakes in a private key.
pub fn authorize_firmware_read(
    device: &DeviceRecord,
    client_subject: Option<&str>,
) -> Result<(), AppError> {
    match client_subject {
        Some(presented) if crate::middleware::is_admin_subject(presented) => Ok(()),
        Some(presented)
            if device
                .subject_dn
                .as_deref()
                .is_some_and(|bound| dn_matches(bound, presented)) =>
        {
            Ok(())
        }
        presented => {
            tracing::warn!(
                device_id = %sanitize_for_log(&device.device_id),
                presented = %sanitize_for_log(presented.unwrap_or("<none>")),
                "firmware read rejected: certificate is neither admin nor the device's own"
            );
            Err(AppError::SubjectMismatch {
                device_id: device.device_id.clone(),
            })
        }
    }
}

/// Register a new device from a JSON request body.
pub fn handle_register_device(
    raw_body: Option<&str>,
    store: &dyn DeviceStore,
) -> Result<DeviceResponse, AppError> {
    let body = raw_body
        .filter(|b| !b.trim().is_empty())
        .ok_or(AppError::MissingBody)?;

    let req: RegisterDeviceRequest =
        serde_json::from_str(body).map_err(|e| AppError::InvalidPayload {
            detail: e.to_string(),
            structured: None,
        })?;

    validate::check_device_id(fields::DEVICE_ID, &req.device_id)?;
    validate::check_text(fields::OWNER_ID, &req.owner_id, validate::MAX_OWNER_ID_LEN)?;
    if let Some(dn) = req.subject_dn.as_deref() {
        validate::check_text(fields::SUBJECT_DN, dn, validate::MAX_SUBJECT_DN_LEN)?;
    }

    // Chart style: whatever the request carries, validated; a device
    // registered without a colour is given the first palette slot the
    // fleet is not wearing, and keeps it.
    let mut style = crate::style::validate(DeviceStyle {
        color: req.color,
        line_style: req.line_style,
        marker: req.marker,
    })?;
    if style.color.is_none() {
        style.color = Some(crate::style::auto_color(&store.list_devices()?).token());
    }

    let record = DeviceRecord {
        device_id: req.device_id,
        owner_id: req.owner_id,
        subject_dn: req.subject_dn,
        status: status::ACTIVE.into(),
        created_at: now_rfc3339(),
        power_threshold_ma: req.power_threshold_ma,
        style,
        desired_fw: None,
    };

    let saved = store.put_device(record)?;
    Ok(DeviceResponse::from(saved))
}

/// Set or clear a device's power threshold override from a JSON body
/// (`{"power_threshold_ma": 12000}`; `{}` or `null` clears it). An empty
/// body also clears: the endpoint is single-purpose, so "nothing" can only
/// mean "back to the fleet default".
pub fn handle_set_device_threshold(
    device_id: &str,
    raw_body: Option<&str>,
    store: &dyn DeviceStore,
) -> Result<DeviceResponse, AppError> {
    validate::check_device_id(fields::DEVICE_ID, device_id)?;
    let invalid = |detail: String| AppError::InvalidPayload {
        detail,
        structured: None,
    };
    let req: supervictor_common::models::DeviceThresholdRequest =
        match raw_body.map(str::trim).filter(|b| !b.is_empty()) {
            None => Default::default(),
            Some(body) => {
                // Only an object (or null) is a request; serde would happily
                // read `[]` as an all-default struct, which must not clear.
                let value: serde_json::Value =
                    serde_json::from_str(body).map_err(|e| invalid(e.to_string()))?;
                match value {
                    serde_json::Value::Null => Default::default(),
                    serde_json::Value::Object(_) => {
                        serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?
                    }
                    _ => return Err(invalid("body must be a JSON object".into())),
                }
            }
        };
    let saved = store.set_device_threshold(device_id, req.power_threshold_ma)?;
    Ok(DeviceResponse::from(saved))
}

/// Replace a device's chart style from a JSON body (`{"color": "s3",
/// "line_style": "dashed", "marker": "circle"}`); `{}`, `null` or an empty
/// body resets every part to its default. Same body discipline as the
/// threshold: only an object or null is a request.
pub fn handle_set_device_style(
    device_id: &str,
    raw_body: Option<&str>,
    store: &dyn DeviceStore,
) -> Result<DeviceResponse, AppError> {
    validate::check_device_id(fields::DEVICE_ID, device_id)?;
    let invalid = |detail: String| AppError::InvalidPayload {
        detail,
        structured: None,
    };
    let style: DeviceStyle = match raw_body.map(str::trim).filter(|b| !b.is_empty()) {
        None => Default::default(),
        Some(body) => {
            let value: serde_json::Value =
                serde_json::from_str(body).map_err(|e| invalid(e.to_string()))?;
            match value {
                serde_json::Value::Null => Default::default(),
                serde_json::Value::Object(_) => {
                    serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?
                }
                _ => return Err(invalid("body must be a JSON object".into())),
            }
        }
    };
    let style = crate::style::validate(style)?;
    let saved = store.set_device_style(device_id, style)?;
    Ok(DeviceResponse::from(saved))
}

/// Look up a single device by its identifier.
pub fn handle_get_device(
    device_id: &str,
    store: &dyn DeviceStore,
) -> Result<DeviceResponse, AppError> {
    let device = store.get_device(device_id)?;
    match device {
        Some(d) => Ok(DeviceResponse::from(d)),
        None => Err(AppError::DeviceNotFound {
            device_id: device_id.to_string(),
        }),
    }
}

/// List all registered devices.
pub fn handle_list_devices(store: &dyn DeviceStore) -> Result<Vec<DeviceResponse>, AppError> {
    let devices = store.list_devices()?;
    Ok(devices.into_iter().map(DeviceResponse::from).collect())
}

/// Retrieve the most recent uplinks for a device, up to `limit`.
pub fn handle_get_device_uplinks(
    device_id: &str,
    store: &dyn DeviceStore,
    limit: usize,
) -> Result<Vec<UplinkRecord>, AppError> {
    store.get_uplinks(device_id, limit)
}
