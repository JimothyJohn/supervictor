use heapless::String as HString;
use serde::Serialize;

// https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/GET
pub fn get_request(host: &str, path: Option<&str>) -> HString<128> {
    let mut request = HString::<128>::new();
    // Use the provided path or default to "/"
    let path = path.unwrap_or("/");

    // Use the path in the request line instead of hardcoded "/"
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

/* Process a complete HTTP response, extract the JSON body, and parse it
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
*/
