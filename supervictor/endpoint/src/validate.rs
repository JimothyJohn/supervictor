//! Field-level validation for inbound payloads.
//!
//! serde guarantees shape and types; these checks guarantee the values are
//! ones the rest of the system can hold: the firmware's identifier buffer is
//! 64 bytes (`HString<64>`, see docs/TELEMETRY.md), identifiers are record
//! keys and log fields, so they may not be empty or carry control characters
//! (log injection), and free-text fields get a sane upper bound so a single
//! request cannot park megabytes in a row.

use crate::error::AppError;
use supervictor_common::fields;
use supervictor_common::levels;

/// Maximum byte length of a device identifier — the firmware's buffer size.
pub const MAX_DEVICE_ID_LEN: usize = 64;
/// Maximum byte length of an owner identifier.
pub const MAX_OWNER_ID_LEN: usize = 128;
/// Maximum byte length of a certificate subject DN.
pub const MAX_SUBJECT_DN_LEN: usize = 1024;
/// Most samples one `levels` burst may carry (ten minutes at 1 Hz).
pub const MAX_LEVELS_SAMPLES: usize = 600;
/// Longest sample spacing in a burst, milliseconds (one hour).
pub const MAX_LEVELS_MS: u32 = 3_600_000;

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

/// Validate a sub-interval burst: `levels` must be a whole number of
/// samples (`levels::COUNT` bytes each), capped at [`MAX_LEVELS_SAMPLES`],
/// and must come with a positive `levels_ms` no longer than
/// [`MAX_LEVELS_MS`]; `levels_ms` alone is meaningless and rejected too.
/// An absent burst (empty `levels`, no `levels_ms`) is always fine.
pub fn check_levels(levels: &[u8], levels_ms: Option<u32>) -> Result<(), AppError> {
    let (field, reason) = if levels.is_empty() {
        match levels_ms {
            None => return Ok(()),
            Some(_) => (fields::UPLINK_LEVELS_MS, "requires levels"),
        }
    } else if !levels.len().is_multiple_of(levels::COUNT) {
        (
            fields::UPLINK_LEVELS,
            "length must be a multiple of the register count",
        )
    } else if levels.len() / levels::COUNT > MAX_LEVELS_SAMPLES {
        (fields::UPLINK_LEVELS, "too many samples")
    } else {
        match levels_ms {
            None => (fields::UPLINK_LEVELS_MS, "required with levels"),
            Some(0) => (fields::UPLINK_LEVELS_MS, "must be positive"),
            Some(ms) if ms > MAX_LEVELS_MS => (fields::UPLINK_LEVELS_MS, "too long"),
            Some(_) => return Ok(()),
        }
    };
    Err(AppError::InvalidPayload {
        detail: format!("{field}: {reason}"),
        structured: Some(serde_json::json!({
            "field": field,
            "reason": reason,
            "registers": levels::COUNT,
            "max_samples": MAX_LEVELS_SAMPLES,
            "max_levels_ms": MAX_LEVELS_MS,
        })),
    })
}
