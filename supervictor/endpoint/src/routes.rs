use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::middleware::ClientSubject;
use crate::store::DeviceStore;
use supervictor_common::routes as wire;

/// Shared application state: a thread-safe reference to the active store backend.
pub type AppState = Arc<dyn DeviceStore>;

/// Build the axum [`Router`] with all API routes and tracing middleware.
pub fn router(store: Arc<dyn DeviceStore>) -> Router {
    Router::new()
        .route(wire::HEALTH, get(health))
        .route(wire::ROOT, get(hello).post(uplink))
        .route(wire::DEVICES, get(list_devices).post(register_device))
        .route(wire::DEVICE_PATTERN, get(get_device))
        .route(wire::DEVICE_UPLINKS_PATTERN, get(get_device_uplinks))
        .with_state(store)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn hello(ClientSubject(subject): ClientSubject) -> Json<serde_json::Value> {
    let resp = handlers::handle_hello(subject);
    Json(serde_json::to_value(resp).unwrap())
}

async fn uplink(
    State(store): State<AppState>,
    ClientSubject(subject): ClientSubject,
    body: String,
) -> Result<Json<serde_json::Value>, crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_uplink(body_opt, subject, Some(store.as_ref()), false)?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

async fn register_device(
    State(store): State<AppState>,
    body: String,
) -> Result<(StatusCode, Json<serde_json::Value>), crate::error::AppError> {
    let body_opt = if body.is_empty() {
        None
    } else {
        Some(body.as_str())
    };
    let resp = handlers::handle_register_device(body_opt, store.as_ref())?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(resp).unwrap()),
    ))
}

async fn list_devices(
    State(store): State<AppState>,
) -> Result<Json<serde_json::Value>, crate::error::AppError> {
    let resp = handlers::handle_list_devices(store.as_ref())?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

async fn get_device(
    State(store): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, crate::error::AppError> {
    let resp = handlers::handle_get_device(&device_id, store.as_ref())?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

async fn get_device_uplinks(
    State(store): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, crate::error::AppError> {
    let resp = handlers::handle_get_device_uplinks(&device_id, store.as_ref(), 10)?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}
