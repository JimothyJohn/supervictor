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
