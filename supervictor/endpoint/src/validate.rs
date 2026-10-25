//! Field-level validation for inbound payloads.
//!
//! serde guarantees shape and types; these checks guarantee the values are
//! ones the rest of the system can hold: the firmware's identifier buffer is
//! 64 bytes (`HString<64>`, see docs/TELEMETRY.md), identifiers are record
//! keys and log fields, so they may not be empty or carry control characters
//! (log injection), and free-text fields get a sane upper bound so a single
//! request cannot park megabytes in a row.

use crate::error::AppError;

/// Maximum byte length of a device identifier — the firmware's buffer size.
pub const MAX_DEVICE_ID_LEN: usize = 64;
/// Maximum byte length of an owner identifier.
pub const MAX_OWNER_ID_LEN: usize = 128;
/// Maximum byte length of a certificate subject DN.
pub const MAX_SUBJECT_DN_LEN: usize = 1024;

/// Reject a field value that is empty, too long, or contains control
/// characters. `field` names the offending JSON key in the error detail.
pub fn check_text(field: &str, value: &str, max_len: usize) -> Result<(), AppError> {
    let reason = if value.trim().is_empty() {
        "must not be empty"
    } else if value.len() > max_len {
        "too long"
    } else if value.chars().any(char::is_control) {
        "must not contain control characters"
    } else {
        return Ok(());
    };
    Err(AppError::InvalidPayload {
        detail: format!("{field}: {reason}"),
        structured: Some(serde_json::json!({
            "field": field,
            "reason": reason,
            "max_len": max_len,
        })),
    })
}

/// Validate a device identifier (uplink `id` / registration `device_id`).
pub fn check_device_id(field: &str, value: &str) -> Result<(), AppError> {
    check_text(field, value, MAX_DEVICE_ID_LEN)
}
