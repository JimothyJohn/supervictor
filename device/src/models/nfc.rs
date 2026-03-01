use heapless::String as HString;
use serde::{Deserialize, Serialize};

/// NFC exchange configuration with sensible defaults.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NfcConfig {
    pub device_id: HString<32>,
    pub max_payload_bytes: u16,
    pub timeout_ms: u16,
    pub retry_count: u8,
}

impl NfcConfig {
    pub fn default_with_id(device_id: &str) -> Self {
        Self {
            device_id: device_id.try_into().unwrap_or_else(|_| HString::new()),
            max_payload_bytes: 128,
            timeout_ms: 1000,
            retry_count: 3,
        }
    }
}

/// A single NFC data record exchanged with an external device.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NfcRecord {
    /// Hex-encoded UID of the external NFC tag/device (e.g. "04A2B3C4D5E6F7")
    pub uid: HString<16>,
    /// Record type: "text", "uri", or "device_info"
    pub record_type: HString<16>,
    /// Record payload
    pub payload: HString<128>,
}

#[cfg(test)]
#[path = "nfc_tests.rs"]
mod tests;
