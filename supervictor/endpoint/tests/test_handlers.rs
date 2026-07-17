mod common;

use supervictor_endpoint::handlers;

#[test]
fn hello_without_subject() {
    let resp = handlers::handle_hello(None);
    assert_eq!(resp.message, "Hello from Supervictor!");
    assert!(resp.client_subject.is_none());
}

#[test]
fn hello_with_subject() {
    let resp = handlers::handle_hello(Some("CN=device1".into()));
    assert_eq!(resp.client_subject.as_deref(), Some("CN=device1"));
}

#[test]
fn uplink_valid_payload() {
    let store = common::test_store();
    let body = r#"{"id": "dev-1", "current": 42}"#;
    let resp = handlers::handle_uplink(Some(body), None, Some(store.as_ref()), false).unwrap();
    assert_eq!(resp.device_id, "dev-1");
    assert_eq!(resp.current, 42);
    assert_eq!(resp.message, "Uplink received");
}

#[test]
fn uplink_empty_body() {
    let err = handlers::handle_uplink(None, None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn uplink_whitespace_body() {
    let err = handlers::handle_uplink(Some("  "), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn uplink_invalid_json() {
    let err = handlers::handle_uplink(Some("{bad"), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn uplink_wrong_schema() {
    let err = handlers::handle_uplink(Some(r#"{"foo": "bar"}"#), None, None, false).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn uplink_with_client_subject() {
    let resp = handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 10}"#),
        Some("CN=test".into()),
        None,
        false,
    )
    .unwrap();
    assert_eq!(resp.client_subject.as_deref(), Some("CN=test"));
}

#[test]
fn uplink_stores_record() {
    let store = common::test_store();
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 99}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    assert_eq!(uplinks[0].payload["current"], 99);
}

/// Every telemetry field the device sends is persisted; the id is not
/// duplicated into the payload (it's the record key), and fields the
/// device omitted don't appear as nulls.
#[test]
fn uplink_stores_full_telemetry_payload() {
    let store = common::test_store();
    let body = r#"{
        "id": "dev-1", "current": -5, "fw": "0.2.0",
        "seq": 17, "uptime_s": 86400, "interval_s": 300,
        "heap_free": 73728, "rssi_dbm": -67,
        "temp_c": 23.5, "humidity_pct": 41.25,
        "accel_g": [0.5, -0.25, 1.0],
        "gyro_dps": [1.5, 0.0, -1.5],
        "mag_ut": [22.5, -4.25, 40.0]
    }"#;
    handlers::handle_uplink(Some(body), None, Some(store.as_ref()), false).unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    let payload = uplinks[0].payload.as_object().unwrap();
    assert!(payload.get("id").is_none(), "id must not be duplicated");
    assert_eq!(payload["current"], -5);
    assert_eq!(payload["fw"], "0.2.0");
    assert_eq!(payload["seq"], 17);
    assert_eq!(payload["uptime_s"], 86400);
    assert_eq!(payload["interval_s"], 300);
    assert_eq!(payload["heap_free"], 73728);
    assert_eq!(payload["rssi_dbm"], -67);
    assert_eq!(payload["temp_c"], 23.5);
    assert_eq!(payload["humidity_pct"], 41.25);
    assert_eq!(payload["accel_g"], serde_json::json!([0.5, -0.25, 1.0]));
    assert_eq!(payload["gyro_dps"], serde_json::json!([1.5, 0.0, -1.5]));
    assert_eq!(payload["mag_ut"], serde_json::json!([22.5, -4.25, 40.0]));
}

/// A minimal (pre-telemetry) uplink stores a minimal payload — absent
/// Options never serialize as null into storage.
#[test]
fn uplink_minimal_payload_stays_minimal() {
    let store = common::test_store();
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 1}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    let payload = uplinks[0].payload.as_object().unwrap();
    assert_eq!(
        payload.len(),
        1,
        "only 'current' expected, got: {payload:?}"
    );
    assert_eq!(payload["current"], 1);
}

#[test]
fn register_device_success() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    let resp = handlers::handle_register_device(Some(body), store.as_ref()).unwrap();
    assert_eq!(resp.device_id, "dev-1");
    assert_eq!(resp.status, "active");
}

#[test]
fn register_device_duplicate() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    handlers::handle_register_device(Some(body), store.as_ref()).unwrap();
    let err = handlers::handle_register_device(Some(body), store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("already exists"), "got: {msg}");
}

#[test]
fn register_device_empty_body() {
    let store = common::test_store();
    let err = handlers::handle_register_device(None, store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("missing request body"), "got: {msg}");
}

#[test]
fn register_device_invalid_json() {
    let store = common::test_store();
    let err = handlers::handle_register_device(Some("{bad"), store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("invalid payload"), "got: {msg}");
}

#[test]
fn get_device_found() {
    let store = common::test_store();
    let body = r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#;
    handlers::handle_register_device(Some(body), store.as_ref()).unwrap();

    let resp = handlers::handle_get_device("dev-1", store.as_ref()).unwrap();
    assert_eq!(resp.device_id, "dev-1");
}

#[test]
fn get_device_not_found() {
    let store = common::test_store();
    let err = handlers::handle_get_device("nonexistent", store.as_ref()).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("not found"), "got: {msg}");
}

#[test]
fn list_devices_empty() {
    let store = common::test_store();
    let resp = handlers::handle_list_devices(store.as_ref()).unwrap();
    assert!(resp.is_empty());
}

#[test]
fn list_devices_with_data() {
    let store = common::test_store();
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#),
        store.as_ref(),
    )
    .unwrap();
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-2", "owner_id": "owner-2"}"#),
        store.as_ref(),
    )
    .unwrap();

    let resp = handlers::handle_list_devices(store.as_ref()).unwrap();
    assert_eq!(resp.len(), 2);
}

#[test]
fn get_device_uplinks_empty() {
    let store = common::test_store();
    let resp = handlers::handle_get_device_uplinks("dev-1", store.as_ref(), 10).unwrap();
    assert!(resp.is_empty());
}

#[test]
fn full_roundtrip() {
    let store = common::test_store();

    // Register device
    handlers::handle_register_device(
        Some(r#"{"device_id": "dev-1", "owner_id": "owner-1"}"#),
        store.as_ref(),
    )
    .unwrap();

    // Send uplink
    handlers::handle_uplink(
        Some(r#"{"id": "dev-1", "current": 42}"#),
        None,
        Some(store.as_ref()),
        false,
    )
    .unwrap();

    // Fetch uplinks
    let uplinks = handlers::handle_get_device_uplinks("dev-1", store.as_ref(), 10).unwrap();
    assert_eq!(uplinks.len(), 1);
    assert_eq!(uplinks[0].payload["current"], 42);
}
