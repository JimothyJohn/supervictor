use crate::models::{FastApiResponse, HttpJsonExtractor};
use heapless::String as HString;
use serde_json_core::{de::Error, from_str};

pub fn config_esp() {
    esp_println::logger::init_logger_from_env();
    // TODO: Optimize this once able
    esp_alloc::heap_allocator!(size: 72 * 1024);
}

/// Process a complete HTTP response, extract the JSON body, and parse it
pub fn process_http_response<const N: usize>(
    http_response: &HString<N>,
) -> Result<FastApiResponse, Error> {
    // Extract the JSON body using the HttpJsonExtractor trait
    if let Some(json_body) = http_response.extract_json() {
        // Directly parse into FastApiResponse
        match from_str::<FastApiResponse>(json_body) {
            Ok((response, _)) => Ok(response),
            Err(e) => Err(e),
        }
    } else {
        Err(Error::InvalidType)
    }
}
