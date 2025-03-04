use esp_println::println;
use heapless::String as HString;
use serde_json_core::{de::Error, from_str};

pub fn config_esp() {
    esp_println::logger::init_logger_from_env();
    esp_alloc::heap_allocator!(size: 72 * 1024);
}

// Define your struct that matches the expected JSON structure
// Using a custom deserializer for string values
#[derive(Debug)]
pub struct MyResponse {
    pub message: HString<64>,
}

// Function to parse JSON string into MyResponse struct
fn parse_json(json_buffer: &str) -> Result<MyResponse, Error> {
    println!("Attempting to parse JSON: {}", json_buffer);

    // Define a temporary struct for deserialization
    #[derive(serde::Deserialize)]
    struct TempResponse<'a> {
        message: &'a str,
    }

    // Parse the JSON into the temporary struct
    match from_str::<TempResponse>(json_buffer) {
        Ok((temp, _)) => {
            // Convert to our actual response type
            let mut message = HString::<64>::new();
            // Use match for more explicit error handling
            match message.push_str(temp.message) {
                Ok(_) => {
                    // String was added successfully
                }
                Err(_) => {
                    // String was too long and got truncated
                    // You could add a debug message here if desired
                    println!("Note: Message was truncated to fit in buffer");
                }
            }

            let response = MyResponse { message };
            println!("Parsed JSON: {:?}", response);
            Ok(response)
        }
        Err(e) => {
            println!("JSON parsing error: {:?}", e);
            Err(e)
        }
    }
}

/// Extract the JSON body from an HTTP response
///
/// This function takes a complete HTTP response (headers + body) and extracts
/// just the JSON body portion for parsing.
fn extract_json_from_http<const N: usize>(http_response: &HString<N>) -> Option<&str> {
    // Find the header/body separator
    // Or maybe just try finding the first open bracket?
    if let Some(body_start) = http_response.find("\r\n\r\n") {
        // Get just the headers to find content length
        let headers = &http_response[..body_start];

        // Find content length to verify we have the complete body
        let content_length = if let Some(cl_pos) = headers.to_lowercase().find("content-length: ") {
            let cl_start = cl_pos + "content-length: ".len();
            let cl_end = headers[cl_start..]
                .find("\r\n")
                .map_or(headers.len(), |pos| cl_start + pos);
            headers[cl_start..cl_end].parse::<usize>().unwrap_or(0)
        } else {
            0
        };

        // Calculate body start position (after the \r\n\r\n)
        let body_start_pos = body_start + 4;

        // Check if we have the complete body
        if http_response.len() >= body_start_pos + content_length {
            return Some(&http_response[body_start_pos..]);
        }
    }

    // If we couldn't find the separator or don't have the complete body, return None
    None
}

/// Process a complete HTTP response, extract the JSON body, and parse it
pub fn process_http_response<const N: usize>(
    http_response: &HString<N>,
) -> Result<MyResponse, Error> {
    // Extract the JSON body
    if let Some(json_body) = extract_json_from_http(http_response) {
        println!("JSON body: {}", json_body);
        parse_json(json_body)
    } else {
        println!("Failed to extract JSON body from HTTP response");
        Err(Error::InvalidType)
    }
}
