use crate::models::nfc::{NfcConfig, NfcRecord};
use heapless::String as HString;

fn make_config() -> NfcConfig {
    NfcConfig::default_with_id("sv-device-01")
}

fn make_record() -> NfcRecord {
    NfcRecord {
        uid: "04A2B3C4D5E6F7".try_into().unwrap(),
        record_type: "text".try_into().unwrap(),
        payload: "hello-nfc".try_into().unwrap(),
    }
}

// ── NfcConfig defaults ──────────────────────────────────────

#[test]
fn config_default_values() {
    let cfg = make_config();
    assert_eq!(cfg.device_id.as_str(), "sv-device-01");
    assert_eq!(cfg.max_payload_bytes, 128);
    assert_eq!(cfg.timeout_ms, 1000);
    assert_eq!(cfg.retry_count, 3);
}

#[test]
fn config_default_empty_id_on_overflow() {
    let long_id = "A".repeat(33);
    let cfg = NfcConfig::default_with_id(&long_id);
    assert!(cfg.device_id.is_empty());
}

// ── NfcConfig serialization ─────────────────────────────────

#[test]
fn config_serialize_basic() {
    let cfg = make_config();
    let json: HString<256> = serde_json_core::to_string(&cfg).unwrap();
    assert!(json.contains(r#""device_id":"sv-device-01""#));
    assert!(json.contains(r#""max_payload_bytes":128"#));
    assert!(json.contains(r#""timeout_ms":1000"#));
    assert!(json.contains(r#""retry_count":3"#));
}

#[test]
fn config_serialize_custom_values() {
    let cfg = NfcConfig {
        device_id: "custom".try_into().unwrap(),
        max_payload_bytes: 256,
        timeout_ms: 5000,
        retry_count: 0,
    };
    let json: HString<256> = serde_json_core::to_string(&cfg).unwrap();
    assert!(json.contains(r#""max_payload_bytes":256"#));
    assert!(json.contains(r#""timeout_ms":5000"#));
    assert!(json.contains(r#""retry_count":0"#));
}

// ── NfcConfig deserialization ───────────────────────────────

#[test]
fn config_deserialize_basic() {
    let json = r#"{"device_id":"d1","max_payload_bytes":64,"timeout_ms":500,"retry_count":2}"#;
    let (cfg, _): (NfcConfig, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(cfg.device_id.as_str(), "d1");
    assert_eq!(cfg.max_payload_bytes, 64);
    assert_eq!(cfg.timeout_ms, 500);
    assert_eq!(cfg.retry_count, 2);
}

#[test]
fn config_deserialize_reordered_fields() {
    let json = r#"{"retry_count":1,"timeout_ms":100,"max_payload_bytes":32,"device_id":"rev"}"#;
    let (cfg, _): (NfcConfig, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(cfg.device_id.as_str(), "rev");
    assert_eq!(cfg.retry_count, 1);
}

// ── NfcConfig roundtrip ─────────────────────────────────────

#[test]
fn config_roundtrip() {
    let original = make_config();
    let json: HString<256> = serde_json_core::to_string(&original).unwrap();
    let (recovered, _): (NfcConfig, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(recovered.device_id.as_str(), original.device_id.as_str());
    assert_eq!(recovered.max_payload_bytes, original.max_payload_bytes);
    assert_eq!(recovered.timeout_ms, original.timeout_ms);
    assert_eq!(recovered.retry_count, original.retry_count);
}

// ── NfcConfig clone ─────────────────────────────────────────

#[test]
fn config_clone_is_independent() {
    let original = make_config();
    let mut cloned = original.clone();
    cloned.timeout_ms = 9999;
    assert_eq!(original.timeout_ms, 1000);
    assert_eq!(cloned.timeout_ms, 9999);
}

// ── NfcConfig capacity limits ───────────────────────────────

#[test]
fn config_device_id_at_max_capacity() {
    let id: HString<32> = core::iter::repeat_n('X', 32).collect();
    let cfg = NfcConfig {
        device_id: id,
        max_payload_bytes: 128,
        timeout_ms: 1000,
        retry_count: 3,
    };
    assert_eq!(cfg.device_id.len(), 32);
}

// ── NfcRecord serialization ─────────────────────────────────

#[test]
fn record_serialize_basic() {
    let rec = make_record();
    let json: HString<256> = serde_json_core::to_string(&rec).unwrap();
    assert!(json.contains(r#""uid":"04A2B3C4D5E6F7""#));
    assert!(json.contains(r#""record_type":"text""#));
    assert!(json.contains(r#""payload":"hello-nfc""#));
}

#[test]
fn record_serialize_uri_type() {
    let rec = NfcRecord {
        uid: "AABB".try_into().unwrap(),
        record_type: "uri".try_into().unwrap(),
        payload: "https://example.com".try_into().unwrap(),
    };
    let json: HString<256> = serde_json_core::to_string(&rec).unwrap();
    assert!(json.contains(r#""record_type":"uri""#));
    assert!(json.contains(r#""payload":"https://example.com""#));
}

#[test]
fn record_serialize_device_info_type() {
    let rec = NfcRecord {
        uid: "CCDD".try_into().unwrap(),
        record_type: "device_info".try_into().unwrap(),
        payload: "fw=1.0.0".try_into().unwrap(),
    };
    let json: HString<256> = serde_json_core::to_string(&rec).unwrap();
    assert!(json.contains(r#""record_type":"device_info""#));
}

#[test]
fn record_serialize_empty_payload() {
    let rec = NfcRecord {
        uid: "0000".try_into().unwrap(),
        record_type: "text".try_into().unwrap(),
        payload: HString::new(),
    };
    let json: HString<256> = serde_json_core::to_string(&rec).unwrap();
    assert!(json.contains(r#""payload":"""#));
}

// ── NfcRecord deserialization ────────────────────────────────

#[test]
fn record_deserialize_basic() {
    let json = r#"{"uid":"AABB","record_type":"text","payload":"data"}"#;
    let (rec, _): (NfcRecord, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(rec.uid.as_str(), "AABB");
    assert_eq!(rec.record_type.as_str(), "text");
    assert_eq!(rec.payload.as_str(), "data");
}

#[test]
fn record_deserialize_reordered_fields() {
    let json = r#"{"payload":"p","uid":"11","record_type":"uri"}"#;
    let (rec, _): (NfcRecord, _) = serde_json_core::from_str(json).unwrap();
    assert_eq!(rec.uid.as_str(), "11");
    assert_eq!(rec.record_type.as_str(), "uri");
    assert_eq!(rec.payload.as_str(), "p");
}

// ── NfcRecord roundtrip ─────────────────────────────────────

#[test]
fn record_roundtrip() {
    let original = make_record();
    let json: HString<256> = serde_json_core::to_string(&original).unwrap();
    let (recovered, _): (NfcRecord, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(recovered.uid.as_str(), original.uid.as_str());
    assert_eq!(recovered.record_type.as_str(), original.record_type.as_str());
    assert_eq!(recovered.payload.as_str(), original.payload.as_str());
}

// ── NfcRecord clone ──────────────────────────────────────────

#[test]
fn record_clone_is_independent() {
    let original = make_record();
    let mut cloned = original.clone();
    cloned.payload = "changed".try_into().unwrap();
    assert_eq!(original.payload.as_str(), "hello-nfc");
    assert_eq!(cloned.payload.as_str(), "changed");
}

// ── NfcRecord capacity limits ────────────────────────────────

#[test]
fn record_uid_at_max_capacity() {
    let uid: HString<16> = core::iter::repeat_n('F', 16).collect();
    let rec = NfcRecord {
        uid,
        record_type: "text".try_into().unwrap(),
        payload: HString::new(),
    };
    assert_eq!(rec.uid.len(), 16);
}

#[test]
fn record_payload_at_max_capacity() {
    let payload: HString<128> = core::iter::repeat_n('P', 128).collect();
    let rec = NfcRecord {
        uid: "AA".try_into().unwrap(),
        record_type: "text".try_into().unwrap(),
        payload,
    };
    assert_eq!(rec.payload.len(), 128);
}

#[test]
fn record_uid_over_capacity_fails() {
    let long_uid = "F".repeat(17);
    let result: Result<HString<16>, _> = long_uid.as_str().try_into();
    assert!(result.is_err());
}

#[test]
fn record_all_fields_empty() {
    let rec = NfcRecord {
        uid: HString::new(),
        record_type: HString::new(),
        payload: HString::new(),
    };
    assert!(rec.uid.is_empty());
    assert!(rec.record_type.is_empty());
    assert!(rec.payload.is_empty());
}
