use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Uplink payload sent by the device. Canonical wire type.
/// Device sends i32; endpoint stores as-is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UplinkMessage {
    pub id: String,
    pub current: i32,
}

/// Device registration request (CLI → endpoint).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterDeviceRequest {
    pub device_id: String,
    pub owner_id: String,
    #[serde(default)]
    pub subject_dn: Option<String>,
}

/// Device info returned by the API (endpoint → CLI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceResponse {
    pub device_id: String,
    pub owner_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_dn: Option<String>,
    pub status: String,
    pub created_at: String,
}
