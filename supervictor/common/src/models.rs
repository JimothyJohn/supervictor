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
    /// Optional per-device power threshold (mA): `current` at or above it is
    /// "on". Absent means the fleet default (`POWER_THRESHOLD_MA`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_threshold_ma: Option<i64>,
    /// Optional chart colour (see [`DeviceStyle::color`]); absent means the
    /// endpoint assigns the first palette slot the fleet is not using.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Optional line style for the device's chart traces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Optional point marker for the device's chart traces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<Marker>,
}

/// How a device's trace is stroked on the dashboard charts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    /// Continuous line (the default).
    #[default]
    Solid,
    /// Long dashes.
    Dashed,
    /// Short dots.
    Dotted,
    /// Dash, dot, dash, dot.
    DashDot,
}

impl LineStyle {
    /// Every style, in menu order.
    pub const ALL: [LineStyle; 4] = [
        LineStyle::Solid,
        LineStyle::Dashed,
        LineStyle::Dotted,
        LineStyle::DashDot,
    ];

    /// Wire token / CSS class (`solid`, `dashed`, `dotted`, `dashdot`).
    pub fn label(self) -> &'static str {
        match self {
            LineStyle::Solid => "solid",
            LineStyle::Dashed => "dashed",
            LineStyle::Dotted => "dotted",
            LineStyle::DashDot => "dashdot",
        }
    }

    /// Parse a wire token; anything else is `None`.
    pub fn parse(token: &str) -> Option<LineStyle> {
        LineStyle::ALL.into_iter().find(|s| s.label() == token)
    }
}

/// Glyph drawn at each plotted point of a device's trace, so two devices
/// that share a colour still read apart.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Marker {
    /// No per-point glyph (the default).
    #[default]
    None,
    /// Filled circle.
    Circle,
    /// Filled square.
    Square,
    /// Filled diamond.
    Diamond,
    /// Filled triangle.
    Triangle,
}

impl Marker {
    /// Every marker, in menu order.
    pub const ALL: [Marker; 5] = [
        Marker::None,
        Marker::Circle,
        Marker::Square,
        Marker::Diamond,
        Marker::Triangle,
    ];

    /// Wire token / CSS class.
    pub fn label(self) -> &'static str {
        match self {
            Marker::None => "none",
            Marker::Circle => "circle",
            Marker::Square => "square",
            Marker::Diamond => "diamond",
            Marker::Triangle => "triangle",
        }
    }

    /// Parse a wire token; anything else is `None`.
    pub fn parse(token: &str) -> Option<Marker> {
        Marker::ALL.into_iter().find(|m| m.label() == token)
    }
}

/// How a device is drawn on the dashboard charts. Stored on the device
/// record; also the body of `PUT /devices/{id}/style` (admin), which
/// replaces the whole style — an absent or `null` field clears that part
/// back to its default. Unknown fields are rejected so a misspelt key
/// cannot silently clear a colour.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeviceStyle {
    /// Trace colour: a palette slot `s0`–`s7` (theme-aware, validated for
    /// both surfaces) or a custom `#rrggbb`. `None` means the endpoint's
    /// positional fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Stroke pattern; `None` is solid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Per-point glyph; `None` is no marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<Marker>,
}

impl DeviceStyle {
    /// True when every part is unset.
    pub fn is_default(&self) -> bool {
        self.color.is_none() && self.line_style.is_none() && self.marker.is_none()
    }
}

/// Per-device power threshold update (`PUT /devices/{id}/threshold`, admin).
/// `null` or an absent field clears the override back to the fleet default.
/// Unknown fields are rejected so a misspelt key cannot silently clear it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeviceThresholdRequest {
    /// Current (mA) at or above which this device counts as powered on.
    #[serde(default)]
    pub power_threshold_ma: Option<i64>,
}

/// Per-device target firmware update (`PUT /devices/{id}/desired_fw`,
/// admin). `null` or an absent field clears it. Unknown fields are
/// rejected so a misspelt key cannot silently clear a rollout target.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DesiredFwRequest {
    /// Version the device should run next; must be an uploaded image for
    /// that device (`PUT /devices/{id}/firmware/{version}`).
    #[serde(default)]
    pub desired_fw: Option<String>,
}

/// What the endpoint knows about one uploaded firmware image. Served at
/// `GET /devices/{id}/firmware/{version}/manifest`; the device checks
/// `chip`, `size` and `sha256` against what it downloads before it
/// activates the slot. Field names are the `fields::FW_*` constants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FirmwareManifest {
    /// Version string (`crate::firmware::Version` shape).
    pub version: String,
    /// Image size in bytes.
    pub size: u32,
    /// SHA-256 of the whole image, lowercase hex (64 chars).
    pub sha256: String,
    /// Chip token (`crate::firmware::ChipId::label`).
    pub chip: String,
    /// RFC 3339 upload time.
    pub uploaded_at: String,
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
    /// Per-device power threshold (mA), if one overrides the fleet default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_threshold_ma: Option<i64>,
    /// Stored chart colour (`s0`–`s7` or `#rrggbb`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Stored line style, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,
    /// Stored point marker, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<Marker>,
    /// Firmware version the device should run next (OTA target), if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desired_fw: Option<String>,
}
