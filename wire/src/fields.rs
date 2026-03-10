// --- UplinkMessage ---

/// Device identifier. Max 64 chars on device side.
pub const UPLINK_ID: &str = "id";

/// Sensor reading. i32 on device, i64 on server (wire-compatible).
pub const UPLINK_CURRENT: &str = "current";

// --- RegisterDeviceRequest / DeviceRecord ---

pub const DEVICE_ID: &str = "device_id";
pub const OWNER_ID: &str = "owner_id";
pub const SUBJECT_DN: &str = "subject_dn";
pub const STATUS: &str = "status";
pub const CREATED_AT: &str = "created_at";
