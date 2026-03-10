use chrono::Utc;

use crate::error::AppError;
use crate::models::{
    DeviceRecord, DeviceResponse, HelloResponse, RegisterDeviceRequest, UplinkMessage,
    UplinkRecord, UplinkResponse,
};
use crate::store::DeviceStore;
use supervictor_wire::status;

/// Return a greeting, optionally echoing the mTLS client subject.
pub fn handle_hello(client_subject: Option<String>) -> HelloResponse {
    HelloResponse {
        message: "Hello from Supervictor!".into(),
        client_subject,
    }
}

/// Parse and persist a device uplink message.
///
/// When `require_registration` is true, the device must exist and be active.
pub fn handle_uplink(
    raw_body: Option<&str>,
    client_subject: Option<String>,
    store: Option<&dyn DeviceStore>,
    require_registration: bool,
) -> Result<UplinkResponse, AppError> {
    let body = raw_body
        .filter(|b| !b.trim().is_empty())
        .ok_or(AppError::MissingBody)?;

    let uplink: UplinkMessage =
        serde_json::from_str(body).map_err(|e| AppError::InvalidPayload {
            detail: e.to_string(),
            structured: None,
        })?;

    if require_registration {
        if let Some(s) = store {
            let device = s.get_device(&uplink.id)?;
            match device {
                Some(d) if d.status == status::ACTIVE => {}
                _ => return Err(AppError::DeviceNotRegistered),
            }
        }
    }

    if let Some(s) = store {
        s.put_uplink(UplinkRecord {
            device_id: uplink.id.clone(),
            received_at: Utc::now().to_rfc3339(),
            payload: serde_json::json!({ "current": uplink.current }),
        })?;
    }

    tracing::info!(device_id = %uplink.id, current = uplink.current, "uplink received");

    Ok(UplinkResponse {
        message: "Uplink received".into(),
        device_id: uplink.id,
        current: uplink.current,
        client_subject,
    })
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

    let record = DeviceRecord {
        device_id: req.device_id,
        owner_id: req.owner_id,
        subject_dn: req.subject_dn,
        status: status::ACTIVE.into(),
        created_at: Utc::now().to_rfc3339(),
    };

    let saved = store.put_device(record)?;
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
