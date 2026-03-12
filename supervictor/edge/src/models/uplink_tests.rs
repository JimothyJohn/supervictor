use crate::models::uplink::{LambdaResponse, UplinkMessage};
use heapless::String as HString;

fn make_uplink(id: &str, current: i32) -> UplinkMessage {
    UplinkMessage {
        id: id.try_into().unwrap(),
        current,
    }
}

fn make_lambda_response() -> LambdaResponse {
    LambdaResponse {
        x_amzn_request_id: "abc-123-def".try_into().unwrap(),
        x_amz_apigw_id: "gw-id-456".try_into().unwrap(),
        x_amzn_trace_id: "Root=1-abc-def".try_into().unwrap(),
        content_type: "application/json".try_into().unwrap(),
        content_length: "42".try_into().unwrap(),
        date: "Thu, 01 Jan 2099 00:00:00".try_into().unwrap(),
        body: "hello-world-body".try_into().unwrap(),
    }
}

// ── UplinkMessage serialization ──────────────────────────────

#[test]
fn uplink_serialize_basic() {
    let msg = make_uplink("dev-1", 42);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert_eq!(json.as_str(), r#"{"id":"dev-1","current":42}"#);
}

#[test]
fn uplink_serialize_empty_id() {
    let msg = UplinkMessage {
        id: HString::new(),
        current: 0,
    };
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert!(json.contains(r#""id":"""#));
    assert!(json.contains(r#""current":0"#));
}

#[test]
fn uplink_serialize_negative_current() {
    let msg = make_uplink("x", -1);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert!(json.contains(r#""current":-1"#));
}

#[test]
fn uplink_serialize_i32_max() {
    let msg = make_uplink("x", i32::MAX);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert!(json.contains("2147483647"));
}

#[test]
fn uplink_serialize_i32_min() {
    let msg = make_uplink("x", i32::MIN);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert!(json.contains("-2147483648"));
}

#[test]
fn uplink_serialize_zero_current() {
    let msg = make_uplink("z", 0);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    assert!(json.contains(r#""current":0"#));
}

// ── UplinkMessage deserialization ────────────────────────────

#[test]
fn uplink_deserialize_basic() {
    let json = r#"{"id":"dev-1","current":42}"#;
    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(msg.id.as_str(), "dev-1");
    assert_eq!(msg.current, 42);
}

#[test]
fn uplink_deserialize_reordered_fields() {
    let json = r#"{"current":99,"id":"reorder"}"#;
    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(msg.id.as_str(), "reorder");
    assert_eq!(msg.current, 99);
}

#[test]
fn uplink_deserialize_negative() {
    let json = r#"{"id":"neg","current":-500}"#;
    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(msg.current, -500);
}

#[test]
fn uplink_deserialize_id_at_64_chars() {
    // Exactly at HString<64> capacity
    let long_id = "A".repeat(64);
    let mut json = HString::<256>::new();
    json.push_str(r#"{"id":""#).unwrap();
    json.push_str(&long_id).unwrap();
    json.push_str(r#"","current":0}"#).unwrap();

    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(msg.id.len(), 64);
}

#[test]
fn uplink_deserialize_id_over_64_fails() {
    let long_id = "A".repeat(65);
    let mut json = HString::<256>::new();
    json.push_str(r#"{"id":""#).unwrap();
    json.push_str(&long_id).unwrap();
    json.push_str(r#"","current":0}"#).unwrap();

    let result: Result<(UplinkMessage, usize), _> = serde_json_core::from_str(json.as_str());
    assert!(result.is_err());
}

// ── UplinkMessage roundtrip ──────────────────────────────────

#[test]
fn uplink_roundtrip_preserves_data() {
    let original = make_uplink("roundtrip-test", -999);
    let json: HString<256> = serde_json_core::to_string(&original).unwrap();
    let (recovered, _): (UplinkMessage, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(recovered.id.as_str(), original.id.as_str());
    assert_eq!(recovered.current, original.current);
}

#[test]
fn uplink_roundtrip_i32_extremes() {
    for val in [i32::MIN, -1, 0, 1, i32::MAX] {
        let msg = make_uplink("rt", val);
        let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
        let (back, _): (UplinkMessage, _) = serde_json_core::from_str(json.as_str()).unwrap();
        assert_eq!(back.current, val);
    }
}

// ── UplinkMessage clone ──────────────────────────────────────

#[test]
fn uplink_clone_is_independent() {
    let original = make_uplink("orig", 100);
    let mut cloned = original.clone();
    cloned.current = 200;
    assert_eq!(original.current, 100);
    assert_eq!(cloned.current, 200);
}

// ── UplinkMessage Debug ──────────────────────────────────────

#[test]
fn uplink_debug_contains_fields() {
    let msg = make_uplink("dbg", 42);
    let dbg = core::format_args!("{:?}", msg);
    // format_args! doesn't allocate, just verify it compiles and doesn't panic
    let _ = dbg;
}

// ── LambdaResponse serde rename ──────────────────────────────

#[test]
fn lambda_response_serialize_uses_renamed_keys() {
    let resp = make_lambda_response();
    let json: HString<512> = serde_json_core::to_string(&resp).unwrap();
    // Verify wire-format key names (hyphenated, not snake_case)
    assert!(json.contains(r#""x-amzn-RequestId""#));
    assert!(json.contains(r#""x-amz-apigw-id""#));
    assert!(json.contains(r#""X-Amzn-Trace-Id""#));
    assert!(json.contains(r#""content-type""#));
    assert!(json.contains(r#""content-length""#));
    // "date" and "body" have no rename
    assert!(json.contains(r#""date""#));
    assert!(json.contains(r#""body""#));
}

#[test]
fn lambda_response_deserialize_wire_format() {
    let json = concat!(
        r#"{"x-amzn-RequestId":"req","x-amz-apigw-id":"gw","#,
        r#""X-Amzn-Trace-Id":"trace","content-type":"ct","#,
        r#""content-length":"42","date":"now","body":"data"}"#
    );
    let (resp, _): (LambdaResponse, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(resp.x_amzn_request_id.as_str(), "req");
    assert_eq!(resp.x_amz_apigw_id.as_str(), "gw");
    assert_eq!(resp.x_amzn_trace_id.as_str(), "trace");
    assert_eq!(resp.content_type.as_str(), "ct");
    assert_eq!(resp.content_length.as_str(), "42");
    assert_eq!(resp.date.as_str(), "now");
    assert_eq!(resp.body.as_str(), "data");
}

#[test]
fn lambda_response_roundtrip() {
    let original = make_lambda_response();
    let json: HString<512> = serde_json_core::to_string(&original).unwrap();
    let (recovered, _): (LambdaResponse, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(
        recovered.x_amzn_request_id.as_str(),
        original.x_amzn_request_id.as_str()
    );
    assert_eq!(recovered.body.as_str(), original.body.as_str());
    assert_eq!(recovered.date.as_str(), original.date.as_str());
}

// ── LambdaResponse capacity limits ───────────────────────────

#[test]
fn lambda_response_all_fields_at_max_capacity() {
    let resp = LambdaResponse {
        x_amzn_request_id: core::iter::repeat_n('R', 64)
            .collect::<HString<64>>(),
        x_amz_apigw_id: core::iter::repeat_n('G', 32)
            .collect::<HString<32>>(),
        x_amzn_trace_id: core::iter::repeat_n('T', 128)
            .collect::<HString<128>>(),
        content_type: core::iter::repeat_n('C', 32)
            .collect::<HString<32>>(),
        content_length: core::iter::repeat_n('L', 8)
            .collect::<HString<8>>(),
        date: core::iter::repeat_n('D', 32)
            .collect::<HString<32>>(),
        body: core::iter::repeat_n('B', 1024)
            .collect::<HString<1024>>(),
    };
    assert_eq!(resp.x_amzn_request_id.len(), 64);
    assert_eq!(resp.x_amz_apigw_id.len(), 32);
    assert_eq!(resp.x_amzn_trace_id.len(), 128);
    assert_eq!(resp.content_type.len(), 32);
    assert_eq!(resp.content_length.len(), 8);
    assert_eq!(resp.date.len(), 32);
    assert_eq!(resp.body.len(), 1024);
}

#[test]
fn lambda_response_empty_fields() {
    let resp = LambdaResponse {
        x_amzn_request_id: HString::new(),
        x_amz_apigw_id: HString::new(),
        x_amzn_trace_id: HString::new(),
        content_type: HString::new(),
        content_length: HString::new(),
        date: HString::new(),
        body: HString::new(),
    };
    assert!(resp.x_amzn_request_id.is_empty());
    assert!(resp.body.is_empty());
}
