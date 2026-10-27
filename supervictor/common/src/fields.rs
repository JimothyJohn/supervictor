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

// Sub-interval burst (optional): 8-bit levels, see `crate::levels`.

/// Flat array of 8-bit register levels, `levels::COUNT` per sample,
/// oldest first; the last sample is the uplink instant.
pub const UPLINK_LEVELS: &str = "levels";
/// Milliseconds between consecutive samples in `levels`.
pub const UPLINK_LEVELS_MS: &str = "levels_ms";

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
/// Per-device power threshold (mA) field name.
pub const POWER_THRESHOLD_MA: &str = "power_threshold_ma";
/// Per-device minimum on-time (seconds) before a device counts as
/// producing; field name.
pub const MIN_ON_S: &str = "min_on_s";
/// Per-device chart colour field name (`s0`–`s7` or `#rrggbb`).
pub const COLOR: &str = "color";
/// Per-device chart line style field name.
pub const LINE_STYLE: &str = "line_style";
/// Per-device chart point marker field name.
pub const MARKER: &str = "marker";
/// Per-device target firmware version field name (OTA control channel):
/// on the device record, in `PUT /devices/{id}/desired_fw`, and echoed in
/// the uplink response when set.
pub const DESIRED_FW: &str = "desired_fw";

// --- FirmwareManifest ---

/// Firmware version string.
pub const FW_VERSION: &str = "version";
/// Image size in bytes.
pub const FW_SIZE: &str = "size";
/// SHA-256 of the whole image, lowercase hex.
pub const FW_SHA256: &str = "sha256";
/// Chip the image targets (`esp32` / `esp32c3`).
pub const FW_CHIP: &str = "chip";
