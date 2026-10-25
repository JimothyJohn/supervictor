use heapless::String as HString;
use heapless::Vec as HVec;
use serde::{Deserialize, Serialize};
use supervictor_common::levels;

/// Firmware version baked in at compile time; reported with every uplink so
/// the fleet dashboard can tell which devices need updating.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Most samples one `levels` burst can carry.
pub const LEVELS_SAMPLES_MAX: usize = 12;
/// Capacity of the flat `levels` array (`LEVELS_SAMPLES_MAX` samples of
/// `levels::COUNT` bytes). Sized into the JSON buffer in `network/http.rs`;
/// `uplink_tests::full_uplink_with_max_id_fits_the_request_buffers` pins it.
pub const LEVELS_CAP: usize = LEVELS_SAMPLES_MAX * levels::COUNT;

/// Telemetry payload sent from the device to the cloud API.
///
/// `no_std` twin of `supervictor_common::models::UplinkMessage`; the field
/// names are the wire contract (`docs/TELEMETRY.md`) and must match the
/// constants in `supervictor/common/src/fields.rs`. Everything past
/// `id`/`current` is optional and left off the wire when `None`, so old
/// firmware and new endpoints (and vice versa) stay compatible.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct UplinkMessage {
    /// Unique identifier for this device or message.
    pub id: HString<64>,
    /// Sensor reading (e.g. current in milliamps).
    pub current: i32,
    /// Firmware version this device is running (see [`FIRMWARE_VERSION`]).
    /// Defaults to empty when parsing pre-fw JSON (backward compatibility).
    #[serde(default)]
    pub fw: HString<16>,

    // --- Device health metadata ---
    /// Uplink counter since boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    /// Seconds since boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_s: Option<u32>,
    /// Configured uplink period in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_s: Option<u32>,
    /// Free heap in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heap_free: Option<u32>,
    /// WiFi RSSI in dBm; absent on wired Ethernet uplinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i8>,

    // --- Environmental readings ---
    /// Ambient temperature in °C.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f32>,
    /// Relative humidity in %RH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<f32>,

    // --- Sub-interval burst (8-bit levels, `supervictor_common::levels`) ---
    /// Flat register levels, `levels::COUNT` per sample, oldest first; the
    /// last sample is this uplink's instant. Empty = no burst on the wire.
    #[serde(default, skip_serializing_if = "HVec::is_empty")]
    pub levels: HVec<u8, LEVELS_CAP>,
    /// Milliseconds between consecutive samples in `levels`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels_ms: Option<u32>,
}

impl UplinkMessage {
    /// Build a minimal uplink stamped with this build's firmware version;
    /// every optional field is absent.
    pub fn new(id: HString<64>, current: i32) -> Self {
        Self {
            id,
            current,
            fw: FIRMWARE_VERSION.try_into().unwrap_or_default(),
            seq: None,
            uptime_s: None,
            interval_s: None,
            heap_free: None,
            rssi_dbm: None,
            temp_c: None,
            humidity_pct: None,
            levels: HVec::new(),
            levels_ms: None,
        }
    }
}

/// Deserialized response from the Lambda-backed API Gateway endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct LambdaResponse {
    /// AWS Lambda request identifier.
    #[serde(rename = "x-amzn-RequestId")]
    pub x_amzn_request_id: HString<64>,
    /// API Gateway internal request identifier.
    #[serde(rename = "x-amz-apigw-id")]
    pub x_amz_apigw_id: HString<32>,
    /// AWS X-Ray trace identifier.
    #[serde(rename = "X-Amzn-Trace-Id")]
    pub x_amzn_trace_id: HString<128>,
    /// MIME type of the response body.
    #[serde(rename = "content-type")]
    pub content_type: HString<32>,
    /// Byte length of the response body as reported by the server.
    #[serde(rename = "content-length")]
    pub content_length: HString<8>,
    /// Date header from the server response.
    pub date: HString<32>,
    /// Raw response body text.
    pub body: HString<1024>,
}
