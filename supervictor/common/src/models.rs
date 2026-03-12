use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Uplink payload sent by the device. Canonical wire type.
/// Device sends i32; endpoint stores as-is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UplinkMessage {
    /// Unique device identifier (max 64 chars on device).
    pub id: String,
    /// Sensor reading value.
    pub current: i32,
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
