use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::models::ErrorResponse;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("missing request body")]
    MissingBody,

    #[error("invalid payload: {detail}")]
    InvalidPayload {
        detail: String,
        structured: Option<serde_json::Value>,
    },

    #[error("device already exists: {device_id}")]
    DeviceAlreadyExists { device_id: String },

    #[error("device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("device not registered or inactive")]
    DeviceNotRegistered,

    #[error("store error: {0}")]
    Store(String),

    #[error("config error: {0}")]
    Config(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            AppError::MissingBody => (
                StatusCode::BAD_REQUEST,
                ErrorResponse {
                    error: "Missing request body".into(),
                    detail: None,
                },
            ),
            AppError::InvalidPayload {
                detail, structured, ..
            } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorResponse {
                    error: "Invalid payload".into(),
                    detail: structured
                        .clone()
                        .or_else(|| Some(serde_json::Value::String(detail.clone()))),
                },
            ),
            AppError::DeviceAlreadyExists { .. } => (
                StatusCode::CONFLICT,
                ErrorResponse {
                    error: "Device already exists".into(),
                    detail: None,
                },
            ),
            AppError::DeviceNotFound { .. } => (
                StatusCode::NOT_FOUND,
                ErrorResponse {
                    error: "Device not found".into(),
                    detail: None,
                },
            ),
            AppError::DeviceNotRegistered => (
                StatusCode::FORBIDDEN,
                ErrorResponse {
                    error: "Device not registered or inactive".into(),
                    detail: None,
                },
            ),
            AppError::Store(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse {
                    error: "Internal server error".into(),
                    detail: None,
                },
            ),
            AppError::Config(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse {
                    error: "Configuration error".into(),
                    detail: None,
                },
            ),
        };

        (status, Json(body)).into_response()
    }
}
