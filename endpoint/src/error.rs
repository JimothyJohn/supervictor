use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::models::ErrorResponse;

/// Application-level error type that maps to HTTP status codes.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Request body was empty or missing.
    #[error("missing request body")]
    MissingBody,

    /// Request payload failed validation or deserialization.
    #[error("invalid payload: {detail}")]
    InvalidPayload {
        /// Human-readable description of the validation failure.
        detail: String,
        /// Optional machine-readable error detail.
        structured: Option<serde_json::Value>,
    },

    /// Attempted to register a device with a duplicate ID.
    #[error("device already exists: {device_id}")]
    DeviceAlreadyExists {
        /// The conflicting device identifier.
        device_id: String,
    },

    /// No device found for the given ID.
    #[error("device not found: {device_id}")]
    DeviceNotFound {
        /// The requested device identifier.
        device_id: String,
    },

    /// Device exists but is not registered or not in active status.
    #[error("device not registered or inactive")]
    DeviceNotRegistered,

    /// Storage backend error (SQLite or DynamoDB).
    #[error("store error: {0}")]
    Store(String),

    /// Configuration/environment error.
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
