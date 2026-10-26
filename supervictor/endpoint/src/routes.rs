use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::middleware::ClientSubject;
use crate::models::{HelloResponse, UplinkRecord, UplinkResponse};
use crate::store::DeviceStore;
use supervictor_common::models::DeviceResponse;
use supervictor_common::routes as wire;

/// Shared application state: the active store backend plus (with the `ui`
/// feature) the live-uplink broadcast channel feeding the dashboard's SSE.
#[derive(Clone)]
pub struct AppState {
    /// Active persistence backend.
    pub store: Arc<dyn DeviceStore>,
    /// Uplink admission policy (see [`RouterOptions`]).
    pub require_registration: bool,
    /// Current (mA) at or above which a device counts as powered on.
    pub power_threshold_ma: i64,
    /// Publisher for dashboard live updates; sends never block.
    #[cfg(feature = "ui")]
    pub events: crate::ui::sse::EventSender,
    /// OTA image blob store; `None` makes the firmware routes answer 503.
    pub firmware: Option<Arc<dyn crate::firmware::FirmwareStore>>,
    /// Bench-only one-shot download faults (`firmware_fault`), in memory.
    pub faults: crate::firmware::FaultTable,
}

/// Runtime policy knobs the router needs beyond the store.
#[derive(Debug, Clone, Copy)]
pub struct RouterOptions {
    /// Reject uplinks from devices that are not registered, `active` and
    /// cert-bound (`REQUIRE_REGISTRATION`). Binding of bound devices is
    /// always enforced.
    pub require_registration: bool,
    /// Current (mA) at or above which a device counts as powered on
    /// (`POWER_THRESHOLD_MA`).
    pub power_threshold_ma: i64,
}

impl Default for RouterOptions {
    fn default() -> Self {
        Self {
            require_registration: false,
            power_threshold_ma: crate::fleet::DEFAULT_POWER_THRESHOLD_MA,
        }
    }
}

/// Build the axum [`Router`] with default options (open ingest: unknown
/// devices may uplink; registered-and-bound devices must match their cert).
pub fn router(store: Arc<dyn DeviceStore>) -> Router {
    router_with(store, RouterOptions::default())
}

/// Build the axum [`Router`] with all API routes and tracing middleware,
/// without a firmware blob store (the OTA routes answer 503).
pub fn router_with(store: Arc<dyn DeviceStore>, options: RouterOptions) -> Router {
    router_with_firmware(store, options, None)
}

/// [`router_with`] plus the OTA firmware blob store.
pub fn router_with_firmware(
    store: Arc<dyn DeviceStore>,
    options: RouterOptions,
    firmware: Option<Arc<dyn crate::firmware::FirmwareStore>>,
) -> Router {
    let state = AppState {
        store,
        require_registration: options.require_registration,
        power_threshold_ma: options.power_threshold_ma,
        #[cfg(feature = "ui")]
        events: crate::ui::sse::channel(),
        firmware,
        faults: crate::firmware::fault_table(),
    };

    // Fleet health API: admin mTLS only — it enumerates the device inventory.
    let fleet_api = Router::new()
        .route(wire::FLEET, get(fleet_status))
        .route(wire::FLEET_SUMMARY, get(fleet_summary))
        .route(wire::FLEET_HISTORY, get(fleet_history))
        // Per-device power threshold: an operator judgement about a machine,
        // so admin-only like the rest of fleet management.
        .route(wire::DEVICE_THRESHOLD_PATTERN, put(set_device_threshold))
        // Per-device chart style: how the dashboard draws the machine.
        .route(wire::DEVICE_STYLE_PATTERN, put(set_device_style))
        // OTA: which version a device should run, and the images it may
        // pull. Uploads are whole images, so the body limit is one slot.
        .route(wire::DEVICE_DESIRED_FW_PATTERN, put(set_desired_fw))
        .route(
            wire::DEVICE_FIRMWARE_PATTERN,
            put(put_firmware)
                .delete(delete_firmware)
                .layer(DefaultBodyLimit::max(crate::firmware::MAX_IMAGE_BYTES)),
        )
        // Bench only: stall a device's next download once (docs/OTA.md
        // "Contract and bench proof"). Admin, like everything that steers OTA.
        .route(
            wire::DEVICE_FIRMWARE_FAULT_PATTERN,
            put(set_firmware_fault).get(get_firmware_fault),
        )
        .route_layer(axum::middleware::from_fn(crate::middleware::require_admin))
        .with_state(state.clone());

    // Firmware downloads: the device's own certificate or an admin
    // (checked per request against the device record, not route-wide).
    let firmware_api = Router::new()
        .route(wire::DEVICE_FIRMWARE_PATTERN, get(get_firmware))
        .route(
            wire::DEVICE_FIRMWARE_MANIFEST_PATTERN,
            get(get_firmware_manifest),
        )
        .with_state(state.clone());

    let router = Router::new()
        .route(wire::HEALTH, get(health))
        .route(wire::ROOT, get(hello).post(uplink))
        .route(wire::DEVICES, get(list_devices).post(register_device))
        .route(wire::DEVICE_PATTERN, get(get_device))
        .route(wire::DEVICE_UPLINKS_PATTERN, get(get_device_uplinks))
        .with_state(state.clone())
        .merge(fleet_api)
        .merge(firmware_api);

    #[cfg(feature = "ui")]
    let router = router.merge(crate::ui::router(state));

    router
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn fleet_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::fleet::FleetDevice>>, crate::error::AppError> {
    Ok(Json(crate::fleet::snapshot_now(
        state.store.as_ref(),
        state.power_threshold_ma,
    )?))
}

async fn fleet_summary(
    State(state): State<AppState>,
) -> Result<Json<crate::fleet::FleetSummary>, crate::error::AppError> {
    let devices = crate::fleet::snapshot_now(state.store.as_ref(), state.power_threshold_ma)?;
    Ok(Json(crate::fleet::summarize(&devices)))
}

/// `?window=` for the history views; anything but a preset is rejected.
#[derive(Debug, serde::Deserialize, Default)]
pub struct HistoryQuery {
    /// Preset token (`15m`, `1h`, `6h`, `24h`, `7d`); default `1h`.
    #[serde(default)]
    pub window: Option<String>,
}

async fn fleet_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<crate::history::History>, crate::error::AppError> {
    let window = crate::history::Window::parse(query.window.as_deref())?;
    let now = crate::time::parse_rfc3339_unix(&crate::time::now_rfc3339()).unwrap_or(0);
    Ok(Json(crate::history::fleet_history(
        state.store.as_ref(),
        window,
        now,
        state.power_threshold_ma,
    )?))
}

async fn set_device_threshold(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    body: String,
) -> Result<Json<DeviceResponse>, crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_set_device_threshold(&device_id, body_opt, state.store.as_ref())?;
    tracing::info!(
        device_id = %crate::middleware::sanitize_for_log(&device_id),
        threshold_ma = ?resp.power_threshold_ma,
        "device threshold set"
    );
    Ok(Json(resp))
}

async fn set_device_style(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    body: String,
) -> Result<Json<DeviceResponse>, crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_set_device_style(&device_id, body_opt, state.store.as_ref())?;
    tracing::info!(
        device_id = %crate::middleware::sanitize_for_log(&device_id),
        color = ?resp.color,
        line_style = ?resp.line_style,
        marker = ?resp.marker,
        "device style set"
    );
    Ok(Json(resp))
}

async fn set_desired_fw(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    body: String,
) -> Result<Json<DeviceResponse>, crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_set_desired_fw(
        &device_id,
        body_opt,
        state.store.as_ref(),
        state.firmware.as_deref(),
    )?;
    tracing::info!(
        device_id = %crate::middleware::sanitize_for_log(&device_id),
        desired_fw = ?resp.desired_fw.as_deref().map(crate::middleware::sanitize_for_log),
        "desired firmware set"
    );
    Ok(Json(resp))
}

/// `(device record, firmware store)` for a firmware route, with the
/// identifiers validated: 422 for an unsafe id or version, 404 for an
/// unknown device, 503 without a blob store.
fn firmware_context(
    state: &AppState,
    device_id: &str,
    version: &str,
) -> Result<
    (
        crate::models::DeviceRecord,
        Arc<dyn crate::firmware::FirmwareStore>,
    ),
    crate::error::AppError,
> {
    crate::firmware::check_identifiers(device_id, version)?;
    let firmware = state
        .firmware
        .clone()
        .ok_or(crate::error::AppError::FirmwareUnavailable)?;
    let device = state.store.get_device(device_id)?.ok_or_else(|| {
        crate::error::AppError::DeviceNotFound {
            device_id: device_id.to_string(),
        }
    })?;
    Ok((device, firmware))
}

async fn put_firmware(
    State(state): State<AppState>,
    Path((device_id, version)): Path<(String, String)>,
    body: Bytes,
) -> Result<
    (
        StatusCode,
        Json<supervictor_common::models::FirmwareManifest>,
    ),
    crate::error::AppError,
> {
    let (_, firmware) = firmware_context(&state, &device_id, &version)?;
    let header = crate::firmware::validate_image(&body)?;
    let manifest = firmware.put(
        &device_id,
        &version,
        header.chip,
        &body,
        &crate::time::now_rfc3339(),
    )?;
    Ok((StatusCode::CREATED, Json(manifest)))
}

async fn delete_firmware(
    State(state): State<AppState>,
    Path((device_id, version)): Path<(String, String)>,
) -> Result<StatusCode, crate::error::AppError> {
    let (_, firmware) = firmware_context(&state, &device_id, &version)?;
    if firmware.delete(&device_id, &version)? {
        tracing::info!(
            device_id = %crate::middleware::sanitize_for_log(&device_id),
            version = %crate::middleware::sanitize_for_log(&version),
            "firmware deleted"
        );
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(crate::error::AppError::FirmwareNotFound { device_id, version })
    }
}

/// Arm (`{"stall_at": N, "stall_secs": S}` → 200 with the fault) or clear
/// (`null`, `{}` or an empty body → 204) the one-shot stall on a device's
/// next image download. Unknown keys are rejected so a typo cannot clear.
async fn set_firmware_fault(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    body: String,
) -> Result<Response, crate::error::AppError> {
    use supervictor_common::models::FirmwareFault;
    crate::validate::check_device_id(supervictor_common::fields::DEVICE_ID, &device_id)?;
    state
        .store
        .get_device(&device_id)?
        .ok_or_else(|| crate::error::AppError::DeviceNotFound {
            device_id: device_id.clone(),
        })?;
    let invalid = |detail: String| crate::error::AppError::InvalidPayload {
        detail,
        structured: None,
    };
    let fault: Option<FirmwareFault> = match body.trim() {
        "" | "null" | "{}" => None,
        text => {
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
            if !value.is_object() {
                return Err(invalid("body must be a JSON object".into()));
            }
            Some(serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?)
        }
    };
    let mut table = crate::firmware::faults(&state.faults);
    match fault {
        Some(fault) => {
            table.insert(device_id.clone(), fault);
            tracing::info!(
                device_id = %crate::middleware::sanitize_for_log(&device_id),
                stall_at = fault.stall_at,
                stall_secs = fault.stall_secs,
                "firmware download fault armed"
            );
            Ok((StatusCode::OK, Json(fault)).into_response())
        }
        None => {
            table.remove(&device_id);
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

/// The fault armed for a device, or 404.
async fn get_firmware_fault(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<supervictor_common::models::FirmwareFault>, crate::error::AppError> {
    crate::validate::check_device_id(supervictor_common::fields::DEVICE_ID, &device_id)?;
    crate::firmware::faults(&state.faults)
        .get(&device_id)
        .copied()
        .map(Json)
        .ok_or(crate::error::AppError::FirmwareFaultNotArmed { device_id })
}

async fn get_firmware_manifest(
    State(state): State<AppState>,
    Path((device_id, version)): Path<(String, String)>,
    ClientSubject(subject): ClientSubject,
) -> Result<Json<supervictor_common::models::FirmwareManifest>, crate::error::AppError> {
    let (device, firmware) = firmware_context(&state, &device_id, &version)?;
    handlers::authorize_firmware_read(&device, subject.as_deref())?;
    let manifest = firmware.manifest(&device_id, &version)?.ok_or_else(|| {
        crate::error::AppError::FirmwareNotFound {
            device_id: device_id.clone(),
            version: version.clone(),
        }
    })?;
    Ok(Json(manifest))
}

/// The image bytes, whole (200) or one `Range` of them (206). The ETag is
/// the manifest's sha256 so a client can tell a replaced image from the
/// one it started downloading.
async fn get_firmware(
    State(state): State<AppState>,
    Path((device_id, version)): Path<(String, String)>,
    ClientSubject(subject): ClientSubject,
    headers: HeaderMap,
) -> Result<Response, crate::error::AppError> {
    use crate::firmware::{parse_range, RangeOutcome};

    let (device, firmware) = firmware_context(&state, &device_id, &version)?;
    handlers::authorize_firmware_read(&device, subject.as_deref())?;
    let not_found = || crate::error::AppError::FirmwareNotFound {
        device_id: device_id.clone(),
        version: version.clone(),
    };
    let manifest = firmware
        .manifest(&device_id, &version)?
        .ok_or_else(not_found)?;
    let total = u64::from(manifest.size);
    let etag = HeaderValue::from_str(&format!("\"{}\"", manifest.sha256)).map_err(|_| {
        crate::error::AppError::Store(crate::store::StoreError::io("etag", "bad sha"))
    })?;
    // `If-Match: "<sha256>"` lets a client resuming a download insist on
    // the image it started with; a re-upload in between is a 412, not a
    // silent splice of two images.
    if let Some(expected) = headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()) {
        let matches = expected
            .split(',')
            .map(|t| t.trim().trim_start_matches("W/").trim_matches('"'))
            .any(|t| t == "*" || t == manifest.sha256);
        if !matches {
            let mut resp = StatusCode::PRECONDITION_FAILED.into_response();
            resp.headers_mut().insert(header::ETAG, etag);
            return Ok(resp);
        }
    }
    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok());

    let (status, start, len, content_range) = match parse_range(range_header, total) {
        RangeOutcome::Whole => (StatusCode::OK, 0, total, None),
        RangeOutcome::Partial(r) => (
            StatusCode::PARTIAL_CONTENT,
            r.start,
            r.len(),
            Some(format!("bytes {}-{}/{}", r.start, r.end, total)),
        ),
        RangeOutcome::Unsatisfiable => {
            let mut resp = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            resp.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{total}"))
                    .unwrap_or(HeaderValue::from_static("bytes */0")),
            );
            resp.headers_mut()
                .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            return Ok(resp);
        }
    };

    let body = firmware
        .read(&device_id, &version, &manifest.sha256, start, len)?
        .ok_or_else(not_found)?;
    // A blob shorter than its manifest is a torn store, not a short range:
    // refuse rather than hand the device something it cannot verify.
    if body.len() as u64 != len {
        return Err(crate::error::AppError::Store(crate::store::StoreError::io(
            "firmware read",
            "blob shorter than its manifest",
        )));
    }
    // One-shot bench stall: same bytes, same length, a pause in the middle.
    let fault = crate::firmware::faults(&state.faults).remove(&device_id);
    let mut resp = match fault {
        Some(fault) => {
            tracing::info!(
                device_id = %crate::middleware::sanitize_for_log(&device_id),
                stall_at = fault.stall_at,
                stall_secs = fault.stall_secs,
                "firmware download: stalling once as armed"
            );
            let stalled = crate::firmware::StalledBody::new(Bytes::from(body), start, fault);
            (status, axum::body::Body::from_stream(stalled)).into_response()
        }
        None => (status, body).into_response(),
    };
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::ETAG, etag);
    h.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    if let Some(cr) = content_range {
        if let Ok(v) = HeaderValue::from_str(&cr) {
            h.insert(header::CONTENT_RANGE, v);
        }
    }
    Ok(resp)
}

async fn hello(ClientSubject(subject): ClientSubject) -> Json<HelloResponse> {
    Json(handlers::handle_hello(subject))
}

async fn uplink(
    State(state): State<AppState>,
    ClientSubject(subject): ClientSubject,
    body: String,
) -> Result<Json<UplinkResponse>, crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let ingested = handlers::ingest_uplink(
        body_opt,
        subject,
        Some(state.store.as_ref()),
        state.require_registration,
    )?;

    // Push to dashboard SSE subscribers; never blocks, no-op with no listeners.
    #[cfg(feature = "ui")]
    let _ = state
        .events
        .send(crate::ui::sse::UplinkEvent::from_record(&ingested.record));

    Ok(Json(ingested.response))
}

async fn register_device(
    State(state): State<AppState>,
    body: String,
) -> Result<(StatusCode, Json<DeviceResponse>), crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_register_device(body_opt, state.store.as_ref())?;
    Ok((StatusCode::CREATED, Json(resp)))
}

async fn list_devices(
    State(state): State<AppState>,
) -> Result<Json<Vec<DeviceResponse>>, crate::error::AppError> {
    let resp = handlers::handle_list_devices(state.store.as_ref())?;
    Ok(Json(resp))
}

async fn get_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<DeviceResponse>, crate::error::AppError> {
    let resp = handlers::handle_get_device(&device_id, state.store.as_ref())?;
    Ok(Json(resp))
}

async fn get_device_uplinks(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<Vec<UplinkRecord>>, crate::error::AppError> {
    let resp = handlers::handle_get_device_uplinks(&device_id, state.store.as_ref(), 10)?;
    Ok(Json(resp))
}
