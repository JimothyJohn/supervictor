use serde::{Deserialize, Serialize};

/// Re-exported wire types shared between endpoint and CLI.
pub use supervictor_wire::models::{DeviceResponse, RegisterDeviceRequest, UplinkMessage};

// --- Domain Records (storage layer) ---

/// Persistent device record stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRecord {
    /// Unique device identifier.
    pub device_id: String,
    /// Owner or tenant identifier.
    pub owner_id: String,
    /// mTLS certificate subject DN, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_dn: Option<String>,
    /// Device lifecycle status (e.g. `active`, `inactive`).
    pub status: String,
    /// ISO 8601 timestamp of device registration.
    pub created_at: String,
}

/// Persistent uplink message record stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UplinkRecord {
    /// Device that sent this uplink.
    pub device_id: String,
    /// ISO 8601 timestamp when the uplink was received.
    pub received_at: String,
    /// Raw JSON payload from the device.
    pub payload: serde_json::Value,
}

// --- Endpoint-only Response Types ---

/// Response body for the `GET /` hello endpoint.
#[derive(Debug, Serialize)]
pub struct HelloResponse {
    /// Greeting message.
    pub message: String,
    /// mTLS client subject DN if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_subject: Option<String>,
}

/// Response body for a successful `POST /` uplink submission.
#[derive(Debug, Serialize)]
pub struct UplinkResponse {
    /// Confirmation message.
    pub message: String,
    /// Device that sent the uplink.
    pub device_id: String,
    /// Current sensor reading from the uplink payload.
    pub current: i32,
    /// mTLS client subject DN if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_subject: Option<String>,
}

/// JSON error response body returned on request failures.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Short error description.
    pub error: String,
    /// Optional structured error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Convert a storage-layer [`DeviceRecord`] into an API [`DeviceResponse`].
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
