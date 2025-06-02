use crate::error::HttpError;
use crate::models::uplink::LambdaResponse;
use heapless::String as HString;
use serde::Serialize;

pub fn get_request(host: &str, path: Option<&str>) -> HString<128> {
    let mut request = HString::<128>::new();
    // Use the provided path or default to "/"
    let path = path.unwrap_or("/");

    // Use the path in the request line instead of hardcoded "/"
    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/GET
    request.push_str("GET ").unwrap();
    request.push_str(path).unwrap();
    request.push_str(" HTTP/1.0").unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("Host: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("User-Agent: ").unwrap();
    request
        .push_str("Uplink/0.1.0 (Platform; ESP32-C3)")
        .unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("Accept: */*").unwrap();
    request.push_str("\r\n\r\n").unwrap();
    request
}

/// Create an HTTP POST request with JSON body
pub fn post_request<T>(host: &str, data: &T, path: Option<&str>) -> HString<512>
where
    T: Serialize,
{
    let mut request = HString::<512>::new();

    // Format the request path
    let endpoint = path.unwrap_or("/");

    // Start building the request
    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/POST
    request.push_str("POST ").unwrap();
    request.push_str(endpoint).unwrap();
    request.push_str(" HTTP/1.1\r\n").unwrap();
    request.push_str("Host: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request
        .push_str("Content-Type: application/json\r\n")
        .unwrap();

    // Serialize the data to JSON
    let json_result = serde_json_core::to_string::<T, 256>(data);

    match json_result {
        Ok(json) => {
            // Add Content-Length header
            request.push_str("Content-Length: ").unwrap();

            // Convert length to string - simplified approach
            let len = json.len();
            // For most HTTP requests, content length will be small
            // This handles up to 5 digits (lengths up to 99999)
            let mut buffer = [0u8; 5];
            let mut i = 0;

            // Handle zero case
            if len == 0 {
                request.push_str("0").unwrap();
            } else {
                // Convert number to digits
                let mut n = len;
                while n > 0 {
                    buffer[i] = (n % 10) as u8 + b'0';
                    n /= 10;
                    i += 1;
                }

                // Add digits in reverse order
                while i > 0 {
                    i -= 1;
                    request.push(buffer[i] as char).unwrap();
                }
            }

            request.push_str("\r\n").unwrap();
            request.push_str("Connection: close").unwrap();
            request.push_str("\r\n\r\n").unwrap();

            // Add the JSON body
            request.push_str(&json).unwrap();
        }
        Err(_) => {
            // Handle serialization error
            request.push_str("Content-Length: 0\r\n\r\n").unwrap();
        }
    }

    request
}

/// Parses the headers and body of an HTTP response
pub fn parse_response(response: &str) -> Result<LambdaResponse, HttpError> {
    // Initialize the LambdaResponse struct with empty heapless Strings.
    // heapless::String::new() creates an empty string with its defined capacity.
    let mut lambda_response = LambdaResponse {
        x_amzn_request_id: HString::new(),
        x_amz_apigw_id: HString::new(),
        x_amzn_trace_id: HString::new(),
        content_type: HString::new(),
        content_length: HString::new(),
        date: HString::new(),
        body: HString::new(),
    };

    // Helper function to push a string slice into a heapless::String field.
    // This function first clears the target HString, then attempts to push the new value.
    // It returns Ok(()) on success, or an HttpError if the value is too large for the HString's capacity.
    fn push_to_hstring<const N: usize>(
        hstring_field: &mut HString<N>,
        value_to_push: &str,
    ) -> Result<(), HttpError> {
        hstring_field.clear(); // Ensure the string is empty before pushing new content.
        match hstring_field.push_str(value_to_push) {
            Ok(_) => Ok(()),
            Err(_) => Err(HttpError::Deserialization), // Using existing Deserialization error variant
        }
    }

    // `response.lines()` creates an iterator over the lines of the response string.
    let mut lines = response.lines();

    // The first line is the HTTP status line (e.g., "HTTP/1.1 200 OK").
    // We skip it as LambdaResponse doesn't store this information.
    if lines.next().is_none() {
        // If there's no first line, the response is empty or malformed.
        return Err(HttpError::Deserialization); // Using existing Deserialization error variant
    }

    // Iterate over the subsequent lines to parse headers.
    // `lines.by_ref()` is used to borrow the iterator, allowing us to continue using `lines`
    // after this loop to parse the body.
    for header_line in lines.by_ref() {
        // An empty line signifies the end of the headers section.
        if header_line.is_empty() {
            break; // Proceed to body parsing.
        }

        // Headers are in "Key: Value" format.
        // `split_once(':')` splits the string at the first occurrence of ':'
        // and returns an Option containing a tuple of (&str_before_colon, &str_after_colon).
        if let Some((key_raw, value_raw)) = header_line.split_once(':') {
            // `trim()` removes leading and trailing whitespace from the key and value.
            let key = key_raw.trim();
            let value = value_raw.trim();

            // Compare header keys case-insensitively and populate the struct.
            // `eq_ignore_ascii_case` is suitable for ASCII-only HTTP headers in a no_std environment.
            if key.eq_ignore_ascii_case("x-amzn-RequestId") {
                push_to_hstring(&mut lambda_response.x_amzn_request_id, value)?;
            } else if key.eq_ignore_ascii_case("x-amz-apigw-id") {
                push_to_hstring(&mut lambda_response.x_amz_apigw_id, value)?;
            } else if key.eq_ignore_ascii_case("X-Amzn-Trace-Id") {
                // HTTP headers are case-insensitive
                push_to_hstring(&mut lambda_response.x_amzn_trace_id, value)?;
            } else if key.eq_ignore_ascii_case("content-type") {
                push_to_hstring(&mut lambda_response.content_type, value)?;
            } else if key.eq_ignore_ascii_case("content-length") {
                push_to_hstring(&mut lambda_response.content_length, value)?;
            } else if key.eq_ignore_ascii_case("date") {
                push_to_hstring(&mut lambda_response.date, value)?;
            }
            // Other headers are ignored.
        } else {
            // Line in header section is not empty and not in "Key: Value" format.
            // This could be considered a malformed header. For now, we'll ignore such lines.
            // Depending on strictness, you might want to return an error:
            // return Err(HttpError::Deserialization); // Using existing Deserialization error variant
        }
    }

    // After the header loop, `lines` iterator is positioned at the first line of the body.
    // Concatenate all remaining lines to form the body.
    lambda_response.body.clear(); // Ensure body string is empty before filling.
    let mut first_body_line = true;
    for body_line_content in lines {
        if !first_body_line {
            // If the body is multi-line, re-insert newline characters that `lines()` consumed.
            if lambda_response.body.push_str("\n").is_err() {
                // Error: Body (plus added newlines) exceeds HString<1024> capacity.
                return Err(HttpError::Deserialization); // Using existing Deserialization error variant
            }
        }

        // Append the current body line to the lambda_response.body field.
        if lambda_response.body.push_str(body_line_content).is_err() {
            // Error: Body content causes overflow.
            return Err(HttpError::Deserialization); // Using existing Deserialization error variant
        }
        first_body_line = false;
    }

    Ok(lambda_response)
}

// AI-Generated comment: Test module, only compiled when running `cargo test`.
#[cfg(test)]
mod tests {
    // AI-Generated comment: Import items from the parent module (http.rs) into the test scope.
    use super::*;
    // AI-Generated comment: Also import necessary items from other modules if needed for tests.
    // AI-Generated comment: For example, you might need the model for post_request.
    use crate::models::UplinkMessage;

    // AI-Generated comment: Basic test function ensuring things compile and run.
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    // AI-Generated comment: Example test for post_request formatting.
    // AI-Generated comment: Tests the structure of the generated HTTP request string.
    #[test]
    fn test_post_request_formatting() {
        // AI-Generated comment: Setup test data.
        let host = "test.host.com";
        let path = Some("/test/path");
        let message = UplinkMessage {
            id: "test-id".try_into().unwrap(),
            current: 99,
        };

        // AI-Generated comment: Call the function under test.
        let request_string = post_request(host, &message, path); // Use .as_str()

        // AI-Generated comment: Perform assertions on the result.
        // AI-Generated comment: Check if the request starts with POST and includes key elements.
        assert!(request_string.starts_with("POST /test/path HTTP/1.1\r\n"));
        assert!(request_string.contains("Host: test.host.com\r\n"));
        assert!(request_string.contains("Content-Type: application/json\r\n"));
        assert!(request_string.contains("\r\n\r\n{\"id\":\"test-id\",\"current\":99}"));
        // Check for body
        // AI-Generated comment: Add more specific checks for headers, content-length etc.
    }

    // AI-Generated comment: Add more tests for get_request, edge cases, different inputs etc.
}
