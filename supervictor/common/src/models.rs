use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Uplink payload sent by the device. Canonical wire type.
/// Device sends i32; endpoint stores as-is.
///
/// Everything past `id`/`current` is optional and omitted from the wire when
/// absent, so old firmware and new endpoints (and vice versa) stay
/// compatible. Permanent device facts (owner, cert subject, registration
/// time) live in the devices reference table, NOT here — the uplink carries
/// only what changes between pings. Units are baked into field names so the
/// stored JSON is self-describing forever. Full schema: `docs/TELEMETRY.md`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UplinkMessage {
    /// Unique device identifier (max 64 chars on device).
    pub id: String,
    /// Sensor reading value.
    pub current: i32,
    /// Firmware version the device is running (absent on pre-fw firmware).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fw: Option<String>,

    // --- Device health metadata (filled by firmware every uplink) ---
    /// Uplink counter since boot; a reset to a low value signals a reboot,
    /// a gap signals dropped pings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    /// Seconds since boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_s: Option<u32>,
    /// Configured uplink period in seconds (t). The user-facing frequency
    /// option is the inverse (f = 1/t); the wire carries the period
    /// canonically. Lets the cloud compute expected-next-ping staleness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_s: Option<u32>,
    /// Free heap in bytes — leak canary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heap_free: Option<u32>,
    /// WiFi RSSI in dBm; absent on wired Ethernet uplinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i8>,

    // --- Environmental placeholders (absent until sensors land) ---
    /// Ambient temperature in °C.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f32>,
    /// Relative humidity in %RH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<f32>,
    /// 9-axis IMU: acceleration [x, y, z] in g.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel_g: Option<[f32; 3]>,
    /// 9-axis IMU: angular rate [x, y, z] in deg/s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gyro_dps: Option<[f32; 3]>,
    /// 9-axis IMU: magnetic field [x, y, z] in µT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mag_ut: Option<[f32; 3]>,

    // --- Sub-interval burst (absent unless the device batches samples) ---
    /// 8-bit register levels, `levels::COUNT` per sample, oldest first;
    /// the last sample is the uplink instant (see `crate::levels`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<u8>,
    /// Milliseconds between consecutive samples in `levels`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels_ms: Option<u32>,
}

/// Device registration request (CLI → endpoint).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterDeviceRequest {
    /// Unique device identifier.
    pub device_id: String,
    /// Owner who is registering the device.
    pub owner_id: String,
    /// Optional X.509 subject DN from the client certificate.
    #[serde(default)]
    pub subject_dn: Option<String>,
}

/// Device info returned by the API (endpoint → CLI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceResponse {
    /// Unique device identifier.
    pub device_id: String,
    /// Owner of the device.
    pub owner_id: String,
    /// X.509 subject DN, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_dn: Option<String>,
    /// Current device status (`"active"` or `"inactive"`).
    pub status: String,
    /// ISO-8601 timestamp of when the device was registered.
    pub created_at: String,
}
