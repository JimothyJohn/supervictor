use supervictor_common::models::{DeviceResponse, RegisterDeviceRequest, UplinkMessage};

// ── UplinkMessage ────────────────────────────────────────────────────

#[test]
fn uplink_roundtrip() {
    let msg = UplinkMessage {
        id: "dev-1".into(),
        current: 42,
        fw: Some("0.1.0".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: UplinkMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn uplink_field_names() {
    let msg = UplinkMessage {
        id: "d".into(),
        current: 0,
        ..Default::default()
    };
    let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert!(val.get("id").is_some(), "expected 'id' key");
    assert!(val.get("current").is_some(), "expected 'current' key");
    // fw is None here and omitted on the wire (skip_serializing_if)
    assert_eq!(val.as_object().unwrap().len(), 2);
}

#[test]
fn uplink_i32_extremes() {
    for v in [i32::MIN, -1, 0, 1, i32::MAX] {
        let msg = UplinkMessage {
            id: "x".into(),
            current: v,
            ..Default::default()
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: UplinkMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.current, v);
    }
}

#[test]
fn uplink_rejects_missing_field() {
    let json = r#"{"id": "dev-1"}"#;
    assert!(serde_json::from_str::<UplinkMessage>(json).is_err());
}

#[test]
fn uplink_rejects_wrong_type() {
    let json = r#"{"id": "dev-1", "current": "not_a_number"}"#;
    assert!(serde_json::from_str::<UplinkMessage>(json).is_err());
}

#[test]
fn uplink_ignores_extra_fields() {
    let json = r#"{"id": "dev-1", "current": 5, "extra": true}"#;
    let msg: UplinkMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.id, "dev-1");
    assert_eq!(msg.current, 5);
}

#[test]
fn uplink_i64_overflow_rejected() {
    let json = format!(r#"{{"id": "x", "current": {}}}"#, i64::MAX);
    assert!(serde_json::from_str::<UplinkMessage>(&json).is_err());
}

// ── UplinkMessage telemetry fields ───────────────────────────────────

/// Full-telemetry uplink survives a JSON roundtrip with every field intact.
#[test]
fn uplink_full_telemetry_roundtrip() {
    let msg = UplinkMessage {
        id: "dev-1".into(),
        current: -42,
        fw: Some("0.1.0".into()),
        seq: Some(u32::MAX),
        uptime_s: Some(86_400),
        interval_s: Some(300),
        heap_free: Some(73_728),
        rssi_dbm: Some(-67),
        temp_c: Some(23.5),
        humidity_pct: Some(41.2),
        accel_g: Some([0.01, -0.02, 1.0]),
        gyro_dps: Some([0.5, -0.5, 0.0]),
        mag_ut: Some([22.1, -4.3, 40.0]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: UplinkMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

/// Absent telemetry is omitted from the wire, not serialized as null —
/// a minimal uplink stays byte-identical to the pre-telemetry format.
#[test]
fn uplink_absent_telemetry_off_the_wire() {
    let msg = UplinkMessage {
        id: "d".into(),
        current: 1,
        ..Default::default()
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, r#"{"id":"d","current":1}"#);
}

/// Old firmware → new endpoint: pre-telemetry JSON parses with all
/// telemetry fields None (forward compat).
#[test]
fn uplink_pre_telemetry_json_parses() {
    let old: UplinkMessage = serde_json::from_str(r#"{"id":"a","current":1}"#).unwrap();
    assert_eq!(old.seq, None);
    assert_eq!(old.uptime_s, None);
    assert_eq!(old.interval_s, None);
    assert_eq!(old.heap_free, None);
    assert_eq!(old.rssi_dbm, None);
    assert_eq!(old.temp_c, None);
    assert_eq!(old.humidity_pct, None);
    assert_eq!(old.accel_g, None);
    assert_eq!(old.gyro_dps, None);
    assert_eq!(old.mag_ut, None);
}

/// Wire field names carry their units — the schema contract with the
/// firmware's heapless twin and with everything reading stored payloads.
#[test]
fn uplink_telemetry_field_names() {
    let msg = UplinkMessage {
        id: "d".into(),
        current: 0,
        fw: Some("1".into()),
        seq: Some(1),
        uptime_s: Some(1),
        interval_s: Some(1),
        heap_free: Some(1),
        rssi_dbm: Some(-1),
        temp_c: Some(0.0),
        humidity_pct: Some(0.0),
        accel_g: Some([0.0; 3]),
        gyro_dps: Some([0.0; 3]),
        mag_ut: Some([0.0; 3]),
    };
    let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
    let obj = val.as_object().unwrap();
    use supervictor_common::fields;
    for key in [
        fields::UPLINK_ID,
        fields::UPLINK_CURRENT,
        fields::UPLINK_FW,
        fields::UPLINK_SEQ,
        fields::UPLINK_UPTIME_S,
        fields::UPLINK_INTERVAL_S,
        fields::UPLINK_HEAP_FREE,
        fields::UPLINK_RSSI_DBM,
        fields::UPLINK_TEMP_C,
        fields::UPLINK_HUMIDITY_PCT,
        fields::UPLINK_ACCEL_G,
        fields::UPLINK_GYRO_DPS,
        fields::UPLINK_MAG_UT,
    ] {
        assert!(obj.contains_key(key), "expected '{key}' on the wire");
    }
    assert_eq!(obj.len(), 13);
}

/// IMU vectors reject wrong arity — [x, y] is a malformed packet, not a
/// silently-padded one.
#[test]
fn uplink_imu_vector_wrong_arity_rejected() {
    let json = r#"{"id":"d","current":0,"accel_g":[0.1,0.2]}"#;
    assert!(serde_json::from_str::<UplinkMessage>(json).is_err());
    let json = r#"{"id":"d","current":0,"accel_g":[0.1,0.2,0.3,0.4]}"#;
    assert!(serde_json::from_str::<UplinkMessage>(json).is_err());
}

// ── RegisterDeviceRequest ────────────────────────────────────────────

#[test]
fn register_roundtrip() {
    let req = RegisterDeviceRequest {
        device_id: "dev-1".into(),
        owner_id: "owner-1".into(),
        subject_dn: Some("CN=dev-1".into()),
        power_threshold_ma: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn register_field_names() {
    let req = RegisterDeviceRequest {
        device_id: "d".into(),
        owner_id: "o".into(),
        subject_dn: None,
        power_threshold_ma: None,
    };
    let val: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert!(val.get("device_id").is_some());
    assert!(val.get("owner_id").is_some());
    assert!(val.get("subject_dn").is_some());
}

#[test]
fn register_subject_dn_defaults_to_none() {
    let json = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    let req: RegisterDeviceRequest = serde_json::from_str(json).unwrap();
    assert!(req.subject_dn.is_none());
}

#[test]
fn register_with_subject_dn() {
    let json = r#"{"device_id": "d", "owner_id": "o", "subject_dn": "CN=test"}"#;
    let req: RegisterDeviceRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.subject_dn.as_deref(), Some("CN=test"));
}

#[test]
fn register_rejects_missing_device_id() {
    let json = r#"{"owner_id": "o"}"#;
    assert!(serde_json::from_str::<RegisterDeviceRequest>(json).is_err());
}

#[test]
fn register_rejects_missing_owner_id() {
    let json = r#"{"device_id": "d"}"#;
    assert!(serde_json::from_str::<RegisterDeviceRequest>(json).is_err());
}

// ── DeviceResponse ───────────────────────────────────────────────────

#[test]
fn device_response_roundtrip() {
    let resp = DeviceResponse {
        device_id: "dev-1".into(),
        owner_id: "owner-1".into(),
        subject_dn: Some("CN=dev-1".into()),
        status: "active".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
        power_threshold_ma: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: DeviceResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn device_response_omits_none_subject_dn() {
    let resp = DeviceResponse {
        device_id: "d".into(),
        owner_id: "o".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "t".into(),
        power_threshold_ma: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        val.get("subject_dn").is_none(),
        "None fields should be omitted"
    );
}

#[test]
fn device_response_includes_some_subject_dn() {
    let resp = DeviceResponse {
        device_id: "d".into(),
        owner_id: "o".into(),
        subject_dn: Some("CN=x".into()),
        status: "active".into(),
        created_at: "t".into(),
        power_threshold_ma: None,
    };
    let val: serde_json::Value = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["subject_dn"], "CN=x");
}

#[test]
fn device_response_field_names() {
    let resp = DeviceResponse {
        device_id: "d".into(),
        owner_id: "o".into(),
        subject_dn: Some("s".into()),
        status: "active".into(),
        created_at: "t".into(),
        power_threshold_ma: None,
    };
    let val: serde_json::Value = serde_json::to_value(&resp).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("device_id"));
    assert!(obj.contains_key("owner_id"));
    assert!(obj.contains_key("subject_dn"));
    assert!(obj.contains_key("status"));
    assert!(obj.contains_key("created_at"));
    assert_eq!(obj.len(), 5);
}

#[test]
fn device_response_deserializes_without_subject_dn() {
    let json = r#"{
        "device_id": "d",
        "owner_id": "o",
        "status": "active",
        "created_at": "t"
    }"#;
    // subject_dn missing from JSON — should deserialize with None
    let resp: DeviceResponse = serde_json::from_str(json).unwrap();
    assert!(resp.subject_dn.is_none());
}

// ── Cross-crate contract ─────────────────────────────────────────────

#[test]
fn device_uplink_json_compatible_with_endpoint() {
    // Simulate what the device sends (i32 range)
    let device_json = r#"{"id": "sensor-1", "current": 100}"#;
    let parsed: UplinkMessage = serde_json::from_str(device_json).unwrap();
    assert_eq!(parsed.id, "sensor-1");
    assert_eq!(parsed.current, 100);
}

#[test]
fn cli_register_json_compatible_with_endpoint() {
    // Simulate what the CLI sends
    let req = RegisterDeviceRequest {
        device_id: "dev-1".into(),
        owner_id: "owner-1".into(),
        subject_dn: Some("CN=dev-1,O=supervictor".into()),
        power_threshold_ma: None,
    };
    let json = serde_json::to_string(&req).unwrap();

    // Endpoint deserializes the same JSON
    let parsed: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.device_id, "dev-1");
    assert_eq!(parsed.subject_dn.as_deref(), Some("CN=dev-1,O=supervictor"));
}

#[test]
fn uplink_fw_wire_compat_both_directions() {
    // Old firmware → new endpoint: no fw field parses as None.
    let old: UplinkMessage = serde_json::from_str(r#"{"id":"a","current":1}"#).unwrap();
    assert_eq!(old.fw, None);
    // New firmware → old readers: fw serializes as a plain extra field.
    let new = UplinkMessage {
        id: "a".into(),
        current: 1,
        fw: Some("1.2.3".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&new).unwrap();
    assert!(json.contains(r#""fw":"1.2.3""#));
}
