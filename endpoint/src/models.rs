use serde::{Deserialize, Serialize};

// Re-export wire types as canonical API types
pub use supervictor_wire::models::{DeviceResponse, RegisterDeviceRequest, UplinkMessage};

// --- Domain Records (storage layer) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: String,
    pub owner_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_dn: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UplinkRecord {
    pub device_id: String,
    pub received_at: String,
    pub payload: serde_json::Value,
}

// --- Endpoint-only Response Types ---

#[derive(Debug, Serialize)]
pub struct HelloResponse {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UplinkResponse {
    pub message: String,
    pub device_id: String,
    pub current: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl From<DeviceRecord> for DeviceResponse {
    fn from(r: DeviceRecord) -> Self {
        Self {
            device_id: r.device_id,
            owner_id: r.owner_id,
            subject_dn: r.subject_dn,
            status: r.status,
            created_at: r.created_at,
        }
    }
}
