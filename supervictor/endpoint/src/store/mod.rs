/// Store backend factory for runtime selection.
pub mod factory;
/// SQLite-backed device store (feature-gated).
#[cfg(feature = "sqlite")]
pub mod sqlite;

/// DynamoDB-backed device store (feature-gated).
#[cfg(feature = "dynamo")]
pub mod dynamo;

use crate::error::AppError;
use crate::models::{DeviceRecord, UplinkRecord};

/// Unix-seconds expiry for an uplink received at `received_at` (RFC 3339)
/// with a retention of `ttl_days`. `None` when TTL is disabled (0 days) or
/// the timestamp doesn't parse — callers store the row without an expiry
/// rather than failing the write; losing retention on one row beats losing
/// the row.
pub fn uplink_expires_at_unix(received_at: &str, ttl_days: u32) -> Option<u64> {
    if ttl_days == 0 {
        return None;
    }
    let received = crate::time::parse_rfc3339_unix(received_at)?;
    Some(received + u64::from(ttl_days) * 86_400)
}

#[cfg(test)]
mod ttl_tests {
    use super::uplink_expires_at_unix;

    #[test]
    fn thirty_days_after_received_at() {
        // 2026-07-16T00:00:00Z = 1784160000
        let expires = uplink_expires_at_unix("2026-07-16T00:00:00+00:00", 30).unwrap();
        assert_eq!(expires, 1_784_160_000 + 30 * 86_400);
    }

    #[test]
    fn zero_days_disables_expiry() {
        assert_eq!(uplink_expires_at_unix("2026-07-16T00:00:00+00:00", 0), None);
    }

    #[test]
    fn unparseable_timestamp_yields_no_expiry_not_a_panic() {
        assert_eq!(uplink_expires_at_unix("not-a-timestamp", 30), None);
        assert_eq!(uplink_expires_at_unix("", 30), None);
    }

    #[test]
    fn max_ttl_does_not_overflow() {
        // u32::MAX days on a far-future timestamp stays within u64.
        let expires = uplink_expires_at_unix("2100-01-01T00:00:00Z", u32::MAX);
        assert!(expires.is_some());
    }

    #[test]
    fn accepts_the_stores_own_timestamp_format() {
        // now_rfc3339() emits fractional seconds + "+00:00"; the TTL parse
        // must accept exactly what put_uplink receives.
        let stamped = crate::time::now_rfc3339();
        assert!(
            uplink_expires_at_unix(&stamped, 30).is_some(),
            "failed to parse own timestamp: {stamped}"
        );
    }
}

/// Typed failure channel for storage backends. Not-found and conflict are
/// already first-class [`AppError`] variants; this classifies everything
/// else so callers can branch (and log/alert) without string matching.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Backend engine or I/O failure (SQLite error, DynamoDB service error).
    Io {
        /// Operation that failed, e.g. `"get_device query"`.
        op: &'static str,
        /// Backend-reported detail.
        detail: String,
    },
    /// Payload (de)serialization failure.
    Serde {
        /// Operation that failed.
        op: &'static str,
        /// Serializer-reported detail.
        detail: String,
    },
    /// A shared lock was poisoned by a panicking writer.
    Poisoned,
}

impl StoreError {
    /// Backend I/O failure during `op`.
    pub fn io(op: &'static str, detail: impl core::fmt::Display) -> Self {
        StoreError::Io {
            op,
            detail: detail.to_string(),
        }
    }

    /// Serialization failure during `op`.
    pub fn serde(op: &'static str, detail: impl core::fmt::Display) -> Self {
        StoreError::Serde {
            op,
            detail: detail.to_string(),
        }
    }
}

impl core::fmt::Display for StoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StoreError::Io { op, detail } => write!(f, "{op}: {detail}"),
            StoreError::Serde { op, detail } => write!(f, "{op} (serde): {detail}"),
            StoreError::Poisoned => f.write_str("store lock poisoned"),
        }
    }
}

/// Trait abstracting device and uplink persistence.
///
/// Implementations must be `Send + Sync` for use as shared axum state.
pub trait DeviceStore: Send + Sync {
    /// Insert a new device record. Returns an error if the device ID already exists.
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError>;
    /// Retrieve a device by its identifier, or `None` if not found.
    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError>;
    /// List all registered devices.
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError>;
    /// Persist an uplink message.
    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError>;
    /// Retrieve the most recent uplinks for a device, up to `limit`.
    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
    /// Update a device's lifecycle status, returning the updated record.
    /// Errors with [`AppError::DeviceNotFound`] if the device does not exist.
    fn set_device_status(&self, device_id: &str, status: &str) -> Result<DeviceRecord, AppError>;

    /// Set (`Some`) or clear (`None`) a device's power threshold override
    /// and its minimum on-time, both at once; returns the updated record,
    /// `DeviceNotFound` for an unknown id. The caller validates the values.
    fn set_device_threshold(
        &self,
        device_id: &str,
        threshold_ma: Option<i64>,
        min_on_s: Option<u32>,
    ) -> Result<DeviceRecord, AppError>;
    /// Set (`Some`) or clear (`None`) the firmware version a device should
    /// run next; returns the updated record, `DeviceNotFound` for an
    /// unknown id. The caller validates the version and that an image
    /// exists — the store only remembers the string.
    fn set_desired_fw(
        &self,
        device_id: &str,
        desired_fw: Option<&str>,
    ) -> Result<DeviceRecord, AppError>;
    /// Replace a device's chart style (every part; unset parts clear);
    /// returns the updated record, `DeviceNotFound` for an unknown id.
    fn set_device_style(
        &self,
        device_id: &str,
        style: crate::models::DeviceStyle,
    ) -> Result<DeviceRecord, AppError>;
    /// Most recent uplink per device (devices that never uplinked are
    /// absent). One query on SQLite; per-device on DynamoDB (see impl notes).
    fn latest_uplinks(&self) -> Result<Vec<UplinkRecord>, AppError>;
    /// One device's uplinks received at or after `since` (RFC 3339, compared
    /// as the fixed-width UTC strings the store writes), oldest first, at
    /// most `limit` rows. Feeds the history charts.
    fn device_uplinks_since(
        &self,
        device_id: &str,
        since: &str,
        limit: usize,
    ) -> Result<Vec<UplinkRecord>, AppError>;
    /// Every device's uplinks received at or after `since`, oldest first,
    /// at most `limit` rows in total (the oldest `limit` — a truncated
    /// window is reported by the caller, not silently shortened at the
    /// recent end where the reader is looking).
    fn uplinks_since(&self, since: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
}
