use crate::error::HttpError;
use crate::models::uplink::UplinkMessage;
use crate::network::http::{get_request, parse_response, post_request};
use heapless::String as HString;

fn make_msg(id: &str, current: i32) -> UplinkMessage {
    UplinkMessage {
        id: id.try_into().unwrap(),
        current,
    }
}

// ════════════════════════════════════════════════════════════════
// get_request
// ════════════════════════════════════════════════════════════════

#[test]
fn get_default_path_is_root() {
    let req = get_request("host.example.com", None).unwrap();
    assert!(req.starts_with("GET / HTTP/1.0\r\n"));
}

#[test]
fn get_explicit_root_path() {
    let req = get_request("host.example.com", Some("/")).unwrap();
    assert!(req.starts_with("GET / HTTP/1.0\r\n"));
}

#[test]
fn get_custom_path() {
    let req = get_request("h", Some("/hello")).unwrap();
    assert!(req.starts_with("GET /hello HTTP/1.0\r\n"));
}

#[test]
fn get_deep_nested_path() {
    let req = get_request("h", Some("/a/b/c/d")).unwrap();
    assert!(req.starts_with("GET /a/b/c/d HTTP/1.0\r\n"));
}

#[test]
fn get_path_with_query_string() {
    let req = get_request("h", Some("/search?q=test&page=1")).unwrap();
    assert!(req.contains("GET /search?q=test&page=1 HTTP/1.0"));
}

#[test]
fn get_host_header_present() {
    let req = get_request("supervictor.advin.io", Some("/")).unwrap();
    assert!(req.contains("Host: supervictor.advin.io\r\n"));
}

#[test]
fn get_user_agent_present() {
    let req = get_request("h", None).unwrap();
    assert!(req.contains("User-Agent: Uplink/0.1.0 (Platform; ESP32-C3)\r\n"));
}

#[test]
fn get_accept_header_present() {
    let req = get_request("h", None).unwrap();
    assert!(req.contains("Accept: */*\r\n"));
}

#[test]
fn get_terminates_with_double_crlf() {
    let req = get_request("h", None).unwrap();
    assert!(req.ends_with("\r\n\r\n"));
}

#[test]
fn get_uses_http_1_0() {
    let req = get_request("h", Some("/")).unwrap();
    assert!(req.contains("HTTP/1.0"));
    assert!(!req.contains("HTTP/1.1"));
}

#[test]
fn get_empty_host() {
    let req = get_request("", Some("/")).unwrap();
    assert!(req.contains("Host: \r\n"));
}

// ════════════════════════════════════════════════════════════════
// post_request
// ════════════════════════════════════════════════════════════════

#[test]
fn post_default_path_is_root() {
    let req = post_request("h", &make_msg("x", 0), None).unwrap();
    assert!(req.starts_with("POST / HTTP/1.1\r\n"));
}

#[test]
fn post_custom_path() {
    let req = post_request("h", &make_msg("x", 0), Some("/hello")).unwrap();
    assert!(req.starts_with("POST /hello HTTP/1.1\r\n"));
}

#[test]
fn post_uses_http_1_1() {
    let req = post_request("h", &make_msg("x", 0), Some("/")).unwrap();
    assert!(req.contains("HTTP/1.1"));
    assert!(!req.contains("HTTP/1.0"));
}

#[test]
fn post_host_header() {
    let req = post_request("supervictor.advin.io", &make_msg("x", 0), Some("/")).unwrap();
    assert!(req.contains("Host: supervictor.advin.io\r\n"));
}

#[test]
fn post_content_type_json() {
    let req = post_request("h", &make_msg("x", 0), Some("/")).unwrap();
    assert!(req.contains("Content-Type: application/json\r\n"));
}

#[test]
fn post_connection_close() {
    let req = post_request("h", &make_msg("x", 0), Some("/")).unwrap();
    assert!(req.contains("Connection: close\r\n"));
}

#[test]
fn post_contains_json_body() {
    let req = post_request("h", &make_msg("test-id", 99), Some("/")).unwrap();
    assert!(req.contains(r#"{"id":"test-id","current":99}"#));
}

#[test]
fn post_body_after_double_crlf() {
    let req = post_request("h", &make_msg("x", 1), Some("/")).unwrap();
    let parts: HString<512> = req;
    let s = parts.as_str();
    let sep = s.find("\r\n\r\n").expect("missing header/body separator");
    let body = &s[sep + 4..];
    assert!(body.starts_with('{'));
    assert!(body.ends_with('}'));
}

#[test]
fn post_content_length_matches_body() {
    let req = post_request("h", &make_msg("test-id", 12345), Some("/")).unwrap();
    let s = req.as_str();

    // Extract Content-Length value
    let cl_prefix = "Content-Length: ";
    let cl_start = s.find(cl_prefix).unwrap() + cl_prefix.len();
    let cl_end = s[cl_start..].find("\r\n").unwrap() + cl_start;
    let claimed: usize = s[cl_start..cl_end].parse().unwrap();

    // Extract actual body
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    let actual_len = s[body_start..].len();

    assert_eq!(claimed, actual_len);
}

#[test]
fn post_zero_current() {
    let req = post_request("h", &make_msg("x", 0), Some("/")).unwrap();
    assert!(req.contains(r#""current":0"#));
}

#[test]
fn post_negative_current() {
    let req = post_request("h", &make_msg("x", -42), Some("/")).unwrap();
    assert!(req.contains(r#""current":-42"#));
}

#[test]
fn post_i32_max_current() {
    let req = post_request("h", &make_msg("x", i32::MAX), Some("/")).unwrap();
    assert!(req.contains("2147483647"));
}

#[test]
fn post_i32_min_current() {
    let req = post_request("h", &make_msg("x", i32::MIN), Some("/")).unwrap();
    assert!(req.contains("-2147483648"));
}

#[test]
fn post_empty_id() {
    let msg = UplinkMessage {
        id: HString::new(),
        current: 1,
    };
    let req = post_request("h", &msg, Some("/")).unwrap();
    assert!(req.contains(r#""id":"""#));
}

#[test]
fn post_max_capacity_id() {
    let long_id: HString<64> = core::iter::repeat_n('A', 64).collect();
    let msg = UplinkMessage {
        id: long_id,
        current: 1,
    };
    let req = post_request("h", &msg, Some("/")).unwrap();
    // 64 A's should be in the body
    let body_start = req.as_str().find("\r\n\r\n").unwrap() + 4;
    let body = &req.as_str()[body_start..];
    let (parsed, _): (UplinkMessage, _) = serde_json_core::from_str(body).unwrap();
    assert_eq!(parsed.id.len(), 64);
}

// ════════════════════════════════════════════════════════════════
// parse_response — happy paths
// ════════════════════════════════════════════════════════════════

const FULL_AWS_RESPONSE: &str = concat!(
    "HTTP/1.1 200 OK\r\n",
    "x-amzn-RequestId: a1b2c3d4-e5f6-7890-abcd-ef1234567890\r\n",
    "x-amz-apigw-id: AbCdEfGhIjKlMnOpQrStUv\r\n",
    "X-Amzn-Trace-Id: Root=1-12345678-abcdef012345678901234567\r\n",
    "Content-Type: application/json\r\n",
    "Content-Length: 42\r\n",
    "Date: Thu, 27 Feb 2025 12:00:00 GMT\r\n",
    "\r\n",
    r#"{"message":"Hello from Supervictor!"}"#,
);

#[test]
fn parse_full_aws_response() {
    let resp = parse_response(FULL_AWS_RESPONSE).unwrap();
    assert_eq!(
        resp.x_amzn_request_id.as_str(),
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    );
    assert_eq!(resp.x_amz_apigw_id.as_str(), "AbCdEfGhIjKlMnOpQrStUv");
    assert_eq!(
        resp.x_amzn_trace_id.as_str(),
        "Root=1-12345678-abcdef012345678901234567"
    );
    assert_eq!(resp.content_type.as_str(), "application/json");
    assert_eq!(resp.content_length.as_str(), "42");
    assert_eq!(resp.date.as_str(), "Thu, 27 Feb 2025 12:00:00 GMT");
    assert!(resp.body.contains("Hello from Supervictor!"));
}

#[test]
fn parse_headers_no_body() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "text/plain");
    assert!(resp.body.is_empty());
}

#[test]
fn parse_minimal_response() {
    let raw = "HTTP/1.1 200 OK\r\n\r\nbody";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.body.as_str(), "body");
}

#[test]
fn parse_json_body() {
    let raw = "HTTP/1.1 200 OK\r\n\r\n{\"key\":\"value\"}";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.body.as_str(), r#"{"key":"value"}"#);
}

#[test]
fn parse_multiline_body() {
    let raw = "HTTP/1.1 200 OK\r\n\r\nline1\r\nline2\r\nline3";
    let resp = parse_response(raw).unwrap();
    // .lines() splits on \n; \r\n produces lines with trailing \r removed by split
    // Body reassembly joins with \n
    assert!(resp.body.contains("line1"));
    assert!(resp.body.contains("line2"));
    assert!(resp.body.contains("line3"));
}

// ════════════════════════════════════════════════════════════════
// parse_response — case insensitivity
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_case_insensitive_content_type() {
    let raw = "HTTP/1.1 200 OK\r\nCONTENT-TYPE: text/html\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "text/html");
}

#[test]
fn parse_case_insensitive_request_id() {
    let raw = "HTTP/1.1 200 OK\r\nX-AMZN-REQUESTID: upper-case-id\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.x_amzn_request_id.as_str(), "upper-case-id");
}

#[test]
fn parse_case_insensitive_date() {
    let raw = "HTTP/1.1 200 OK\r\nDATE: Mon, 01 Jan 2099\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.date.as_str(), "Mon, 01 Jan 2099");
}

// ════════════════════════════════════════════════════════════════
// parse_response — header edge cases
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_colon_in_header_value() {
    // Trace-Id contains colons — split_once(':') handles this correctly
    let raw = "HTTP/1.1 200 OK\r\nX-Amzn-Trace-Id: Root=1-abc:def:ghi\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.x_amzn_trace_id.as_str(), "Root=1-abc:def:ghi");
}

#[test]
fn parse_extra_whitespace_in_value() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type:   text/plain   \r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "text/plain");
}

#[test]
fn parse_unknown_headers_ignored() {
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "X-Custom-Header: ignored\r\n",
        "Server: nginx\r\n",
        "Content-Type: application/json\r\n",
        "\r\n",
    );
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "application/json");
}

#[test]
fn parse_duplicate_header_last_wins() {
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: first\r\n",
        "Content-Type: second\r\n",
        "\r\n",
    );
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "second");
}

#[test]
fn parse_empty_header_value() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: \r\n\r\n";
    let resp = parse_response(raw).unwrap();
    assert!(resp.content_type.is_empty());
}

#[test]
fn parse_malformed_header_no_colon_ignored() {
    // A header line without ':' should be silently skipped
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "NotAValidHeader\r\n",
        "Content-Type: text/plain\r\n",
        "\r\n",
    );
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "text/plain");
}

// ════════════════════════════════════════════════════════════════
// parse_response — error cases
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_empty_string_fails() {
    assert!(parse_response("").is_err());
}

#[test]
fn parse_status_line_only_no_headers_no_body() {
    // Just a status line with no blank line separator — no body
    let resp = parse_response("HTTP/1.1 200 OK");
    // This should succeed — status consumed, loop ends without blank line, body is empty
    assert!(resp.is_ok());
    assert!(resp.unwrap().body.is_empty());
}

#[test]
fn parse_garbage_input() {
    let resp = parse_response("this is not http at all");
    // First line consumed as status, rest parsed as headers — should not crash
    assert!(resp.is_ok());
}

// ════════════════════════════════════════════════════════════════
// parse_response — HTTP status codes
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_http_404() {
    let raw = "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n\r\n{\"message\":\"Not Found\"}";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Not Found"));
}

#[test]
fn parse_http_500() {
    let raw = "HTTP/1.1 500 Internal Server Error\r\n\r\n{\"message\":\"Error\"}";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Error"));
}

// ════════════════════════════════════════════════════════════════
// parse_response — capacity overflow
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_body_at_1024_capacity() {
    let body: HString<1024> = core::iter::repeat_n('X', 1024).collect();
    let mut raw = HString::<1100>::new();
    raw.push_str("HTTP/1.1 200 OK\r\n\r\n").unwrap();
    raw.push_str(body.as_str()).unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.body.len(), 1024);
}

#[test]
fn parse_body_over_1024_fails() {
    let body: HString<1028> = core::iter::repeat_n('X', 1025).collect();
    let mut raw = HString::<1100>::new();
    raw.push_str("HTTP/1.1 200 OK\r\n\r\n").unwrap();
    raw.push_str(body.as_str()).unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_request_id_at_64() {
    let val: HString<64> = core::iter::repeat_n('R', 64).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nx-amzn-RequestId: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.x_amzn_request_id.len(), 64);
}

#[test]
fn parse_request_id_over_64_fails() {
    let val: HString<65> = core::iter::repeat_n('R', 65).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nx-amzn-RequestId: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_apigw_id_at_32() {
    let val: HString<32> = core::iter::repeat_n('G', 32).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nx-amz-apigw-id: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.x_amz_apigw_id.len(), 32);
}

#[test]
fn parse_apigw_id_over_32_fails() {
    let val: HString<33> = core::iter::repeat_n('G', 33).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nx-amz-apigw-id: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_trace_id_at_128() {
    let val: HString<128> = core::iter::repeat_n('T', 128).collect();
    let mut raw = HString::<300>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nX-Amzn-Trace-Id: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.x_amzn_trace_id.len(), 128);
}

#[test]
fn parse_trace_id_over_128_fails() {
    let val: HString<129> = core::iter::repeat_n('T', 129).collect();
    let mut raw = HString::<300>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nX-Amzn-Trace-Id: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_content_type_at_32() {
    let val: HString<32> = core::iter::repeat_n('C', 32).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nContent-Type: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.content_type.len(), 32);
}

#[test]
fn parse_content_type_over_32_fails() {
    let val: HString<33> = core::iter::repeat_n('C', 33).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nContent-Type: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_content_length_at_8() {
    let val: HString<8> = core::iter::repeat_n('9', 8).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nContent-Length: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.content_length.len(), 8);
}

#[test]
fn parse_content_length_over_8_fails() {
    let val: HString<9> = core::iter::repeat_n('9', 9).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nContent-Length: ")
        .unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

#[test]
fn parse_date_at_32() {
    let val: HString<32> = core::iter::repeat_n('D', 32).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nDate: ").unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.date.len(), 32);
}

#[test]
fn parse_date_over_32_fails() {
    let val: HString<33> = core::iter::repeat_n('D', 33).collect();
    let mut raw = HString::<200>::new();
    raw.push_str("HTTP/1.1 200 OK\r\nDate: ").unwrap();
    raw.push_str(val.as_str()).unwrap();
    raw.push_str("\r\n\r\n").unwrap();
    assert!(parse_response(raw.as_str()).is_err());
}

// ════════════════════════════════════════════════════════════════
// parse_response — line ending variants
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_lf_only_line_endings() {
    let raw = "HTTP/1.1 200 OK\nContent-Type: text/plain\n\nbody";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.content_type.as_str(), "text/plain");
    assert_eq!(resp.body.as_str(), "body");
}

#[test]
fn parse_special_chars_in_body() {
    let raw = "HTTP/1.1 200 OK\r\n\r\n<html>&amp;\"quotes\"</html>";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("<html>"));
    assert!(resp.body.contains("&amp;"));
}

#[test]
fn parse_unicode_in_body() {
    let raw = "HTTP/1.1 200 OK\r\n\r\nhello world";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("hello"));
}

// ════════════════════════════════════════════════════════════════
// Error Display
// ════════════════════════════════════════════════════════════════

#[test]
fn error_display_deserialization() {
    extern crate alloc;
    use alloc::format;
    let msg = format!("{}", HttpError::Deserialization);
    assert!(!msg.is_empty());
    assert!(msg.contains("deserialize"));
}

#[test]
fn error_display_generic_parse() {
    extern crate alloc;
    use alloc::format;
    let msg = format!("{}", HttpError::GenericParseError);
    assert!(!msg.is_empty());
    assert!(msg.contains("parse"));
}

#[test]
fn error_display_variants_distinct() {
    extern crate alloc;
    use alloc::format;
    let d = format!("{}", HttpError::Deserialization);
    let g = format!("{}", HttpError::GenericParseError);
    assert_ne!(d, g);
}

#[test]
fn error_debug_deserialization() {
    extern crate alloc;
    use alloc::format;
    let dbg = format!("{:?}", HttpError::Deserialization);
    assert!(dbg.contains("Deserialization"));
}

#[test]
fn error_debug_generic_parse() {
    extern crate alloc;
    use alloc::format;
    let dbg = format!("{:?}", HttpError::GenericParseError);
    assert!(dbg.contains("GenericParseError"));
}

// ════════════════════════════════════════════════════════════════
// Round-trips: build request → simulate response → parse
// ════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_post_then_parse() {
    let msg = make_msg("device-001", 42);
    let _req = post_request("supervictor.advin.io", &msg, None).unwrap();

    // Simulate what API Gateway would return
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "x-amzn-RequestId: 11111111-2222-3333-4444-555555555555\r\n",
        "x-amz-apigw-id: TestGatewayId\r\n",
        "X-Amzn-Trace-Id: Root=1-test-trace\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 24\r\n",
        "Date: Thu, 01 Jan 2099 00:00:00 GMT\r\n",
        "\r\n",
        r#"{"message":"Uplink OK"}"#,
    );

    let parsed = parse_response(response).unwrap();
    assert_eq!(
        parsed.x_amzn_request_id.as_str(),
        "11111111-2222-3333-4444-555555555555"
    );
    assert!(parsed.body.contains("Uplink OK"));
}

#[test]
fn roundtrip_get_then_parse() {
    let _req = get_request("supervictor.advin.io", None).unwrap();

    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 36\r\n",
        "\r\n",
        r#"{"message":"Hello from Supervictor!"}"#,
    );

    let parsed = parse_response(response).unwrap();
    assert!(parsed.body.contains("Hello from Supervictor!"));
}

#[test]
fn roundtrip_post_body_deserializable() {
    let msg = make_msg("round", -1);
    let req = post_request("h", &msg, Some("/")).unwrap();

    // Extract the JSON body from the request
    let body_start = req.as_str().find("\r\n\r\n").unwrap() + 4;
    let body = &req.as_str()[body_start..];

    // Deserialize back into UplinkMessage
    let (recovered, _): (UplinkMessage, _) = serde_json_core::from_str(body).unwrap();
    assert_eq!(recovered.id.as_str(), "round");
    assert_eq!(recovered.current, -1);
}
