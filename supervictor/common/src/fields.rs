// --- UplinkMessage ---

/// Device identifier. Max 64 chars on device side.
pub const UPLINK_ID: &str = "id";

/// Sensor reading. i32 on device, i64 on server (wire-compatible).
pub const UPLINK_CURRENT: &str = "current";

/// Firmware version string.
pub const UPLINK_FW: &str = "fw";

// Device health metadata (optional, omitted when absent).

/// Uplink counter since boot.
pub const UPLINK_SEQ: &str = "seq";
/// Seconds since boot.
pub const UPLINK_UPTIME_S: &str = "uptime_s";
/// Configured uplink period in seconds.
pub const UPLINK_INTERVAL_S: &str = "interval_s";
/// Free heap in bytes.
pub const UPLINK_HEAP_FREE: &str = "heap_free";
/// WiFi RSSI in dBm (absent on wired Ethernet).
pub const UPLINK_RSSI_DBM: &str = "rssi_dbm";

// Environmental sensor fields (optional placeholders until sensors land).

/// Ambient temperature in °C.
pub const UPLINK_TEMP_C: &str = "temp_c";
/// Relative humidity in %RH.
pub const UPLINK_HUMIDITY_PCT: &str = "humidity_pct";
/// IMU acceleration [x, y, z] in g.
pub const UPLINK_ACCEL_G: &str = "accel_g";
/// IMU angular rate [x, y, z] in deg/s.
pub const UPLINK_GYRO_DPS: &str = "gyro_dps";
/// IMU magnetic field [x, y, z] in µT.
pub const UPLINK_MAG_UT: &str = "mag_ut";

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
