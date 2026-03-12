// --- UplinkMessage ---

/// Device identifier. Max 64 chars on device side.
pub const UPLINK_ID: &str = "id";

/// Sensor reading. i32 on device, i64 on server (wire-compatible).
pub const UPLINK_CURRENT: &str = "current";

// --- RegisterDeviceRequest / DeviceRecord ---

/// Device identifier field name.
pub const DEVICE_ID: &str = "device_id";
/// Owner identifier field name.
pub const OWNER_ID: &str = "owner_id";
/// X.509 subject distinguished name field name.
pub const SUBJECT_DN: &str = "subject_dn";
/// Device status field name.
pub const STATUS: &str = "status";
/// ISO-8601 creation timestamp field name.
pub const CREATED_AT: &str = "created_at";
