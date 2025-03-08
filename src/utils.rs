use crate::models::{FastApiResponse, HttpJsonExtractor, RequestBody, RequestData};
use esp_println::println;
use heapless::String as HString;
use serde_json_core::{de::Error, from_str};

pub fn config_esp() {
    esp_println::logger::init_logger_from_env();
    // TODO: Optimize this once able
    esp_alloc::heap_allocator!(size: 72 * 1024);
}

// Function to parse JSON string into FastApiResponse struct
fn deserialize_json(json_buffer: &str) -> Result<FastApiResponse, Error> {
    println!("Attempting to parse JSON: {}", json_buffer);

    // Directly parse into FastApiResponse
    match from_str::<FastApiResponse>(json_buffer) {
        Ok((response, _)) => {
            println!("Successfully parsed JSON response");
            Ok(response)
        }
        Err(e) => {
            println!("JSON parsing error: {:?}", e);
            Err(e)
        }
    }
}

/// Serialize a RequestBody struct into a JSON string, wrapped in RequestData
pub fn serialize_json(response: &RequestBody) -> Result<HString<128>, Error> {
    println!("Attempting to serialize: {:?}", response);

    // Create RequestData wrapper
    let request_data = RequestData {
        data: RequestBody {
            body: response.body.clone(),
        },
    };

    // Handle the Result returned by to_string
    match serde_json_core::to_string::<RequestData, 128>(&request_data) {
        Ok(result) => {
            println!("Serialized JSON: {}", result);
            Ok(result)
        }
        Err(e) => {
            println!("Serialization error: {:?}", e);
            Err(Error::InvalidType)
        }
    }
}

/// Process a complete HTTP response, extract the JSON body, and parse it
pub fn process_http_response<const N: usize>(
    http_response: &HString<N>,
) -> Result<FastApiResponse, Error> {
    // Extract the JSON body using the HttpJsonExtractor trait
    if let Some(json_body) = http_response.extract_json() {
        println!("JSON body: {}", json_body);
        deserialize_json(json_body)
    } else {
        println!("Failed to extract JSON body from HTTP response");
        Err(Error::InvalidType)
    }
}
