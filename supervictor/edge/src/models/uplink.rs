use heapless::String as HString;
use serde::{Deserialize, Serialize};

/// Firmware version baked in at compile time; reported with every uplink so
/// the fleet dashboard can tell which devices need updating.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Telemetry payload sent from the device to the cloud API.
///
/// no_std twin of `supervictor_common::models::UplinkMessage` — keep the two
/// in lockstep. Optional fields are omitted from the wire when `None`, so a
/// build that never fills them serializes exactly like pre-telemetry
/// firmware. Units live in the field names; full schema: `docs/TELEMETRY.md`.
/// Worst-case serialized size must fit the JSON buffer in
/// `network/http.rs::post_request` — see `uplink_tests::uplink_worst_case_fits_json_buffer`.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UplinkMessage {
    /// Unique identifier for this device or message.
    pub id: HString<64>,
    /// Sensor reading (e.g. current in milliamps).
    pub current: i32,
    /// Firmware version this device is running (see [`FIRMWARE_VERSION`]).
    /// Defaults to empty when parsing pre-fw JSON (backward compatibility).
    #[serde(default)]
    pub fw: HString<16>,

    // --- Device health metadata (filled by the uplink loop every ping) ---
    /// Uplink counter since boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    /// Seconds since boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_s: Option<u32>,
    /// Configured uplink period in seconds (t; user-facing frequency is 1/t).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_s: Option<u32>,
    /// Free heap in bytes — leak canary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heap_free: Option<u32>,
    /// WiFi RSSI in dBm; `None` on wired Ethernet uplinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i8>,

    // --- Environmental placeholders (`None` until sensors land) ---
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
}

impl UplinkMessage {
    /// Build an uplink stamped with this build's firmware version.
    /// Health metadata and sensor fields start `None`; the uplink loop
    /// fills the health fields before each send.
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
            accel_g: None,
            gyro_dps: None,
            mag_ut: None,
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
