use crate::error::HttpError;
use crate::models::uplink::LambdaResponse;
use heapless::String as HString;
use serde::Serialize;

/// Push a str into a heapless String, mapping capacity errors to HttpError.
fn push<const N: usize>(buf: &mut HString<N>, s: &str) -> Result<(), HttpError> {
    buf.push_str(s).map_err(|_| HttpError::BufferOverflow)
}

/// Build an HTTP/1.1 GET request for the given host and optional path.
///
/// Same dialect as [`post_request`]: HTTP/1.1 + `Connection: close`, since
/// the device is a one-shot client that reads to EOF. Capacity is 192 — the
/// fixed headers are ~100 bytes, leaving room for real hostnames like
/// `staging.supervictor.advin.io` plus a path.
pub fn get_request(host: &str, path: Option<&str>) -> Result<HString<192>, HttpError> {
    let mut request = HString::<192>::new();
    let path = path.unwrap_or("/");

    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/GET
    push(&mut request, "GET ")?;
    push(&mut request, path)?;
    push(&mut request, " HTTP/1.1\r\n")?;
    push(&mut request, "Host: ")?;
    push(&mut request, host)?;
    push(&mut request, "\r\n")?;
    push(
        &mut request,
        "User-Agent: Uplink/0.1.0 (Platform; ESP32)\r\n",
    )?;
    push(&mut request, "Connection: close\r\n")?;
    push(&mut request, "Accept: */*\r\n\r\n")?;
    Ok(request)
}

/// Create an HTTP POST request with JSON body.
///
/// Capacities (mirrored in `config.rs` — that module is embedded-gated, this
/// one is host-testable): 768 bytes of JSON body fits a worst-case
/// full-telemetry UplinkMessage (64-char id, 9 IMU floats, env + health
/// fields and a full 12-sample `levels` burst — pinned in `uplink_tests`);
/// 1024 total leaves ~150 bytes of headers plus slack.
pub fn post_request<T>(host: &str, data: &T, path: Option<&str>) -> Result<HString<1024>, HttpError>
where
    T: Serialize,
{
    let mut request = HString::<1024>::new();
    let endpoint = path.unwrap_or("/");

    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/POST
    push(&mut request, "POST ")?;
    push(&mut request, endpoint)?;
    push(&mut request, " HTTP/1.1\r\n")?;
    push(&mut request, "Host: ")?;
    push(&mut request, host)?;
    push(&mut request, "\r\nContent-Type: application/json\r\n")?;

    // One-shot client: tell HTTP/1.1 servers to close instead of keeping
    // the connection alive while the device blocks on read-to-EOF.
    push(&mut request, "Connection: close\r\n")?;

    // A payload that doesn't fit the buffer is an error, not an empty POST —
    // silently sending Content-Length: 0 would register a bogus uplink.
    let json = serde_json_core::to_string::<T, 768>(data).map_err(|_| HttpError::Serialization)?;

    push(&mut request, "Content-Length: ")?;
    push_usize(&mut request, json.len())?;
    push(&mut request, "\r\n\r\n")?;
    push(&mut request, &json)?;

    Ok(request)
}

/// Write a usize as decimal digits into a heapless String.
fn push_usize<const N: usize>(buf: &mut HString<N>, value: usize) -> Result<(), HttpError> {
    if value == 0 {
        return push(buf, "0");
    }
    let mut digits = [0u8; 10];
    let mut i = 0;
    let mut n = value;
    while n > 0 {
        digits[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        buf.push(digits[i] as char)
            .map_err(|_| HttpError::BufferOverflow)?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::uplink::UplinkMessage;

    #[test]
    fn test_post_request_formatting() {
        let host = "test.host.com";
        let path = Some("/test/path");
        let message = UplinkMessage::new("test-id".try_into().unwrap(), 99);

        let request_string = post_request(host, &message, path).unwrap();

        assert!(request_string.starts_with("POST /test/path HTTP/1.1\r\n"));
        assert!(request_string.contains("Host: test.host.com\r\n"));
        assert!(request_string.contains("Content-Type: application/json\r\n"));
        assert!(request_string.contains(concat!(
            "\r\n\r\n{\"id\":\"test-id\",\"current\":99,\"fw\":\"",
            env!("CARGO_PKG_VERSION"),
            "\"}"
        )));
    }

    /// The body is the serialized struct, and it round-trips. Regression for
    /// the desktop client, which pre-serialized the message and passed the
    /// resulting string in as `data`, so the body went out double-encoded as
    /// a JSON string literal (`"{\"id\":..."`) and the endpoint rejected it.
    #[test]
    fn test_post_request_body_round_trips_as_object() {
        let message = UplinkMessage::new("pi-edge-01".try_into().unwrap(), 7);
        let request = post_request("h", &message, None).unwrap();
        let body = request.split("\r\n\r\n").nth(1).unwrap();
        assert!(body.starts_with('{'), "body is not a JSON object: {body}");
        let (parsed, _) = serde_json_core::from_str::<UplinkMessage>(body).unwrap();
        assert_eq!(parsed.id.as_str(), "pi-edge-01");
        assert_eq!(parsed.current, 7);

        // Passing an already-serialized string is the trap: it becomes a string literal.
        let pre: HString<64> = "{\"id\":\"x\"}".try_into().unwrap();
        let bad = post_request("h", &pre, None).unwrap();
        let bad_body = bad.split("\r\n\r\n").nth(1).unwrap();
        assert!(bad_body.starts_with('"'));
    }

    /// GET and POST speak the same dialect: HTTP/1.1 + Connection: close.
    #[test]
    fn test_get_request_dialect_matches_post() {
        let request = get_request("test.host.com", Some("/hello")).unwrap();
        assert!(request.starts_with("GET /hello HTTP/1.1\r\n"));
        assert!(request.contains("Host: test.host.com\r\n"));
        assert!(request.contains("Connection: close\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }

    /// Production-sized hostname + path must fit the GET buffer.
    #[test]
    fn test_get_request_fits_staging_hostname() {
        let request = get_request("staging.supervictor.advin.io", Some("/hello")).unwrap();
        assert!(request.contains("Host: staging.supervictor.advin.io\r\n"));
    }

    /// Overlong host/path overflows the buffer as an error, not truncation.
    #[test]
    fn test_get_request_overflow_is_an_error() {
        const LONG_HOST: &str = concat!(
            "very-long-subdomain-name-segment.another-long-segment",
            ".yet-another-segment.example-domain-name.com"
        );
        let result = get_request(LONG_HOST, Some("/a/rather/long/path/for/good/measure"));
        assert!(matches!(result, Err(HttpError::BufferOverflow)));
    }

    /// Regression: a payload that exceeds the JSON serialization buffer must
    /// surface an error, never a silent empty-body POST (Content-Length: 0)
    /// that the server would accept as a valid-but-meaningless uplink.
    #[test]
    fn test_post_request_oversized_payload_is_an_error() {
        #[derive(serde::Serialize)]
        struct Oversized {
            data: &'static str,
        }
        // 780 chars > the 768-byte serde-json-core buffer in post_request
        const LONG: &str = concat!(
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz",
        );
        let big = Oversized { data: LONG };

        let result = post_request("host", &big, None);
        assert!(
            matches!(result, Err(HttpError::Serialization)),
            "oversized payload must fail with Serialization error, got {result:?}"
        );
    }
}
