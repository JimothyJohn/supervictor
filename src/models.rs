use heapless::String as HString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Clone)]
pub struct RequestBody {
    pub body: HString<64>,
}

#[derive(Debug, Deserialize)]
pub struct FastApiResponse {
    // Using heapless::Vec to store the array of error details
    // pub detail: heapless::Vec<ErrorDetail, 8>,
    pub message: HString<64>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorDetail {
    #[serde(rename = "type")]
    pub error_type: HString<32>,
    pub loc: heapless::Vec<HString<32>, 4>,
    pub msg: HString<64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<HString<32>>,
}

// Helper trait for extracting JSON from HTTP responses
pub trait HttpJsonExtractor {
    fn extract_json(&self) -> Option<&str>;
}

impl<const N: usize> HttpJsonExtractor for HString<N> {
    fn extract_json(&self) -> Option<&str> {
        // Find the header/body separator
        if let Some(body_start) = self.find("\r\n\r\n") {
            // Calculate body start position (after the \r\n\r\n)
            let body_start_pos = body_start + 4;

            // Return the body portion
            if body_start_pos < self.len() {
                return Some(&self[body_start_pos..]);
            }
        }

        // If we couldn't find the separator, check if the whole string is JSON
        if self.starts_with('{') && self.ends_with('}') {
            return Some(self);
        }

        None
    }
}
