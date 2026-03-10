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
}
