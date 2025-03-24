use heapless::String as HString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Clone)]
pub struct UplinkMessage {
    pub id: HString<64>,
    pub current: i32,
}

#[derive(Debug, Deserialize)]
pub struct FastApiResponse {
    pub message: HString<64>,
}
