use heapless::String as HString;
use serde::{Deserialize, Serialize};

/// Telemetry payload sent from the device to the cloud API.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UplinkMessage {
    /// Unique identifier for this device or message.
    pub id: HString<64>,
    /// Sensor reading (e.g. current in milliamps).
    pub current: i32,
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
