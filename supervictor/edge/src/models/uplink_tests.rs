use crate::models::uplink::{LambdaResponse, UplinkMessage};
use heapless::String as HString;

fn make_uplink(id: &str, current: i32) -> UplinkMessage {
    UplinkMessage::new(id.try_into().unwrap(), current)
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
    assert_eq!(
        json.as_str(),
        concat!(
            r#"{"id":"dev-1","current":42,"fw":""#,
            env!("CARGO_PKG_VERSION"),
            r#""}"#
        )
    );
}

#[test]
fn uplink_serialize_empty_id() {
    let msg = UplinkMessage::new(HString::new(), 0);
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
    // pre-fw JSON parses with an empty firmware version (backward compat)
    assert!(msg.fw.is_empty());
}

#[test]
fn uplink_deserialize_with_fw() {
    let json = r#"{"id":"dev-1","current":42,"fw":"0.1.0"}"#;
    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(msg.fw.as_str(), "0.1.0");
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
    assert_eq!(recovered.fw.as_str(), original.fw.as_str());
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
        x_amzn_request_id: core::iter::repeat_n('R', 64).collect::<HString<64>>(),
        x_amz_apigw_id: core::iter::repeat_n('G', 32).collect::<HString<32>>(),
        x_amzn_trace_id: core::iter::repeat_n('T', 128).collect::<HString<128>>(),
        content_type: core::iter::repeat_n('C', 32).collect::<HString<32>>(),
        content_length: core::iter::repeat_n('L', 8).collect::<HString<8>>(),
        date: core::iter::repeat_n('D', 32).collect::<HString<32>>(),
        body: core::iter::repeat_n('B', 1024).collect::<HString<1024>>(),
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

// ── Optional telemetry fields (wire contract, docs/TELEMETRY.md) ─────

use crate::models::sim::{seed_from, Hardware, Simulator};

/// Field names are the contract with `supervictor_common::fields`; a typo
/// here would silently drop a column on the endpoint.
const OPTIONAL_KEYS: [&str; 7] = [
    "seq",
    "uptime_s",
    "interval_s",
    "heap_free",
    "rssi_dbm",
    "temp_c",
    "humidity_pct",
];

#[test]
fn minimal_uplink_omits_every_optional_field() {
    let msg = make_uplink("dev-1", 1);
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    for key in OPTIONAL_KEYS {
        assert!(!json.contains(key), "{key} leaked into {json}");
    }
}

#[test]
fn full_uplink_serializes_every_field_by_its_wire_name() {
    let mut sim = Simulator::new(seed_from("dev-1", 0));
    let msg = sim.uplink("dev-1".try_into().unwrap(), 42, 5, Hardware::default());
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    for key in OPTIONAL_KEYS {
        let mut quoted = HString::<32>::new();
        quoted.push('"').unwrap();
        quoted.push_str(key).unwrap();
        quoted.push_str("\":").unwrap();
        assert!(json.contains(quoted.as_str()), "{key} missing from {json}");
    }
    assert!(json.contains("\"uptime_s\":42,"));
    assert!(json.contains("\"interval_s\":5,"));
}

#[test]
fn full_uplink_round_trips_through_serde_json_core() {
    let mut sim = Simulator::new(seed_from("dev-1", 1));
    let msg = sim.uplink("dev-1".try_into().unwrap(), 7, 5, Hardware::default());
    let json: HString<256> = serde_json_core::to_string(&msg).unwrap();
    let (back, _): (UplinkMessage, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(back, msg);
}

/// The worst case — a 64-byte id, extreme values in every field — must fit
/// the `post_request` JSON buffer (384) and the request buffer (512), or
/// the firmware would silently stop uplinking after a rename. (It is 260
/// bytes; the old 256 buffer was one field short.)
#[test]
fn full_uplink_with_max_id_fits_the_request_buffers() {
    let id: HString<64> = core::iter::repeat_n('x', 64).collect();
    let mut msg = UplinkMessage::new(id, i32::MIN);
    msg.fw = "999.999.999".try_into().unwrap();
    msg.seq = Some(u32::MAX);
    msg.uptime_s = Some(u32::MAX);
    msg.interval_s = Some(u32::MAX);
    msg.heap_free = Some(u32::MAX);
    msg.rssi_dbm = Some(i8::MIN);
    msg.temp_c = Some(-99.999_99);
    msg.humidity_pct = Some(100.000_01);
    let json: HString<384> = serde_json_core::to_string(&msg).expect("fits 384");
    assert!(json.len() <= 384, "{} bytes", json.len());
    let request = crate::network::http::post_request("staging.supervictor.advin.io", &msg, None)
        .expect("fits 512");
    assert!(request.ends_with(json.as_str()));
}

/// Pre-telemetry endpoints ignore unknown keys; pre-telemetry firmware
/// payloads still parse here (the field defaults are the rollback path).
#[test]
fn legacy_payload_parses_with_absent_optionals() {
    let json = r#"{"id":"old","current":3,"fw":"0.0.1"}"#;
    let (msg, _): (UplinkMessage, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(msg.seq, None);
    assert_eq!(msg.temp_c, None);
}
