use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::models::ErrorResponse;

/// Application-level error type that maps to HTTP status codes.
#[derive(Debug)]
pub enum AppError {
    /// Request body was empty or missing.
    MissingBody,

    /// Request payload failed validation or deserialization.
    InvalidPayload {
        /// Human-readable description of the validation failure.
        detail: String,
        /// Optional machine-readable error detail.
        structured: Option<serde_json::Value>,
    },

    /// Attempted to register a device with a duplicate ID.
    DeviceAlreadyExists {
        /// The conflicting device identifier.
        device_id: String,
    },

    /// No device found for the given ID.
    DeviceNotFound {
        /// The requested device identifier.
        device_id: String,
    },

    /// Device exists but is not registered or not in active status.
    DeviceNotRegistered,

    /// The mTLS client subject does not match the device's registered
    /// `subject_dn` (or no client certificate was presented for a bound
    /// device). The expected DN is never echoed back.
    SubjectMismatch {
        /// Device whose identity the request claimed.
        device_id: String,
    },

    /// Storage backend error (SQLite or DynamoDB), classified.
    Store(crate::store::StoreError),

    /// Configuration/environment error.
    Config(String),
}

// Hand-rolled Display/Error impls, inspired by thiserror's #[error("...")] derive
// (https://github.com/dtolnay/thiserror). The derive only generated these two
// impls; reintroduce thiserror if the error surface grows #[from]/#[source] chains.
impl core::fmt::Display for AppError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AppError::MissingBody => f.write_str("missing request body"),
            AppError::InvalidPayload { detail, .. } => write!(f, "invalid payload: {detail}"),
            AppError::DeviceAlreadyExists { device_id } => {
                write!(f, "device already exists: {device_id}")
            }
            AppError::DeviceNotFound { device_id } => write!(f, "device not found: {device_id}"),
            AppError::DeviceNotRegistered => f.write_str("device not registered or inactive"),
            AppError::SubjectMismatch { device_id } => {
                write!(f, "client certificate does not match device: {device_id}")
            }
            AppError::Store(msg) => write!(f, "store error: {msg}"),
            AppError::Config(msg) => write!(f, "config error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

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
            AppError::SubjectMismatch { .. } => (
                StatusCode::FORBIDDEN,
                ErrorResponse {
                    error: "Client certificate does not match device".into(),
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
