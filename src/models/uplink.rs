use heapless::String as HString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UplinkMessage {
    pub id: HString<64>,
    pub current: i32,
}

// New struct to deserialize the echo response
#[derive(Debug, Serialize, Deserialize)]
pub struct LambdaResponse {
    #[serde(rename = "x-amzn-RequestId")]
    pub x_amzn_request_id: HString<64>,
    #[serde(rename = "x-amz-apigw-id")]
    pub x_amz_apigw_id: HString<32>,
    #[serde(rename = "X-Amzn-Trace-Id")]
    pub x_amzn_trace_id: HString<128>,
    #[serde(rename = "content-type")]
    pub content_type: HString<32>,
    #[serde(rename = "content-length")]
    pub content_length: HString<8>,
    pub date: HString<32>,
    pub body: HString<1024>,
}
