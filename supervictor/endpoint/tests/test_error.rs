use axum::http::StatusCode;
use axum::response::IntoResponse;

use supervictor_endpoint::error::AppError;

fn status_of(err: AppError) -> StatusCode {
    err.into_response().status()
}

fn body_of(err: AppError) -> serde_json::Value {
    let resp = err.into_response();
    let (_, body) = resp.into_parts();
    let bytes = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(axum::body::to_bytes(body, 1024))
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ── Status codes ─────────────────────────────────────────────────────

#[test]
fn missing_body_returns_400() {
    assert_eq!(status_of(AppError::MissingBody), StatusCode::BAD_REQUEST);
}

#[test]
fn invalid_payload_returns_422() {
    assert_eq!(
        status_of(AppError::InvalidPayload {
            detail: "bad".into(),
            structured: None,
        }),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[test]
fn device_already_exists_returns_409() {
    assert_eq!(
        status_of(AppError::DeviceAlreadyExists {
            device_id: "d".into()
        }),
        StatusCode::CONFLICT
    );
}

#[test]
fn device_not_found_returns_404() {
    assert_eq!(
        status_of(AppError::DeviceNotFound {
            device_id: "d".into()
        }),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn device_not_registered_returns_403() {
    assert_eq!(
        status_of(AppError::DeviceNotRegistered),
        StatusCode::FORBIDDEN
    );
}

#[test]
fn store_error_returns_500() {
    assert_eq!(
        status_of(AppError::Store("db failed".into())),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn config_error_returns_500() {
    assert_eq!(
        status_of(AppError::Config("bad config".into())),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

// ── Response bodies ──────────────────────────────────────────────────

#[test]
fn missing_body_error_message() {
    let body = body_of(AppError::MissingBody);
    assert_eq!(body["error"], "Missing request body");
}

#[test]
fn invalid_payload_includes_detail() {
    let body = body_of(AppError::InvalidPayload {
        detail: "missing field".into(),
        structured: None,
    });
    assert_eq!(body["error"], "Invalid payload");
    assert_eq!(body["detail"], "missing field");
}

#[test]
fn invalid_payload_prefers_structured_detail() {
    let structured = serde_json::json!({"field": "current", "reason": "expected i32"});
    let body = body_of(AppError::InvalidPayload {
        detail: "fallback".into(),
        structured: Some(structured.clone()),
    });
    assert_eq!(body["detail"], structured);
}

#[test]
fn device_already_exists_error_message() {
    let body = body_of(AppError::DeviceAlreadyExists {
        device_id: "dev-1".into(),
    });
    assert_eq!(body["error"], "Device already exists");
}

#[test]
fn device_not_found_error_message() {
    let body = body_of(AppError::DeviceNotFound {
        device_id: "dev-1".into(),
    });
    assert_eq!(body["error"], "Device not found");
}

#[test]
fn store_error_hides_internal_detail() {
    let body = body_of(AppError::Store("SQLITE_ERROR: table locked".into()));
    assert_eq!(body["error"], "Internal server error");
    assert!(body.get("detail").is_none());
}

#[test]
fn config_error_hides_internal_detail() {
    let body = body_of(AppError::Config("missing AWS key".into()));
    assert_eq!(body["error"], "Configuration error");
    assert!(body.get("detail").is_none());
}

// ── Display trait ────────────────────────────────────────────────────

#[test]
fn display_includes_context() {
    let err = AppError::DeviceNotFound {
        device_id: "dev-42".into(),
    };
    assert!(format!("{err}").contains("dev-42"));
}

#[test]
fn display_missing_body() {
    let err = AppError::MissingBody;
    assert_eq!(format!("{err}"), "missing request body");
}
