pub mod factory;
#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "dynamo")]
pub mod dynamo;

use crate::error::AppError;
use crate::models::{DeviceRecord, UplinkRecord};

pub trait DeviceStore: Send + Sync {
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError>;
    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError>;
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError>;
    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError>;
    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
}
