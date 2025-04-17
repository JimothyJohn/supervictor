use heapless::String as HString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UplinkMessage {
    pub id: HString<64>,
    pub current: i32,
}

// New struct to deserialize the echo response
#[derive(Debug, Deserialize)]
pub struct EchoResponse {
    pub method: HString<8>,
    pub protocol: HString<8>,
    pub host: HString<64>,
    pub path: HString<64>,
    pub ip: HString<32>,
    pub headers: EchoHeaders,
    // Using Option for fields that might be missing in some responses
    #[serde(rename = "parsedQueryParams")]
    pub parsed_query_params: EchoQueryParams,
    #[serde(rename = "parsedBody")]
    pub parsed_body: Option<UplinkMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EchoHeaders {
    pub host: HString<64>,
    #[serde(rename = "User-Agent")]
    pub user_agent: Option<HString<64>>,
    #[serde(rename = "Content-Length")]
    pub content_length: HString<8>,
    pub accept: HString<32>,
    #[serde(rename = "Content-Type")]
    pub content_type: Option<HString<32>>,
    #[serde(rename = "Accept-Encoding")]
    pub accept_encoding: HString<16>,
}

#[derive(Debug, Deserialize)]
pub struct EchoQueryParams {
    // This is an empty struct since parsedQueryParams is empty in your example
    // If you expect query parameters in the future, add them here
}
