use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
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
    /// Publisher for dashboard live updates; sends never block.
    #[cfg(feature = "ui")]
    pub events: crate::ui::sse::EventSender,
}

/// Runtime policy knobs the router needs beyond the store.
#[derive(Debug, Clone, Copy, Default)]
pub struct RouterOptions {
    /// Reject uplinks from devices that are not registered and `active`
    /// (`REQUIRE_REGISTRATION`). Certificate binding is always enforced.
    pub require_registration: bool,
}

/// Build the axum [`Router`] with default options (open ingest: unknown
/// devices may uplink; registered-and-bound devices must match their cert).
pub fn router(store: Arc<dyn DeviceStore>) -> Router {
    router_with(store, RouterOptions::default())
}

/// Build the axum [`Router`] with all API routes and tracing middleware.
pub fn router_with(store: Arc<dyn DeviceStore>, options: RouterOptions) -> Router {
    let state = AppState {
        store,
        require_registration: options.require_registration,
        #[cfg(feature = "ui")]
        events: crate::ui::sse::channel(),
    };

    // Fleet health API: admin mTLS only — it enumerates the device inventory.
    let fleet_api = Router::new()
        .route(wire::FLEET, get(fleet_status))
        .route(wire::FLEET_SUMMARY, get(fleet_summary))
        .route_layer(axum::middleware::from_fn(crate::middleware::require_admin))
        .with_state(state.clone());

    let router = Router::new()
        .route(wire::HEALTH, get(health))
        .route(wire::ROOT, get(hello).post(uplink))
        .route(wire::DEVICES, get(list_devices).post(register_device))
        .route(wire::DEVICE_PATTERN, get(get_device))
        .route(wire::DEVICE_UPLINKS_PATTERN, get(get_device_uplinks))
        .with_state(state.clone())
        .merge(fleet_api);

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
    Ok(Json(crate::fleet::snapshot_now(state.store.as_ref())?))
}

async fn fleet_summary(
    State(state): State<AppState>,
) -> Result<Json<crate::fleet::FleetSummary>, crate::error::AppError> {
    let devices = crate::fleet::snapshot_now(state.store.as_ref())?;
    Ok(Json(crate::fleet::summarize(&devices)))
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
    let resp = handlers::handle_uplink(
        body_opt,
        subject,
        Some(state.store.as_ref()),
        state.require_registration,
    )?;

    // Push to dashboard SSE subscribers; never blocks, no-op with no listeners.
    #[cfg(feature = "ui")]
    let _ = state.events.send(crate::ui::sse::UplinkEvent {
        device_id: resp.device_id.clone(),
        current: resp.current,
        received_at: crate::time::now_rfc3339(),
    });

    Ok(Json(resp))
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
