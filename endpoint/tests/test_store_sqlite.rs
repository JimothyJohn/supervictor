mod common;

use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};

fn make_device(id: &str) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
    }
}

fn make_uplink(device_id: &str, ts: &str, current: i64) -> UplinkRecord {
    UplinkRecord {
        device_id: device_id.into(),
        received_at: ts.into(),
        payload: serde_json::json!({ "current": current }),
    }
}

#[test]
fn put_and_get_device() {
    let store = common::test_store();
    let device = make_device("dev-1");
    let saved = store.put_device(device.clone()).unwrap();
    assert_eq!(saved.device_id, "dev-1");

    let fetched = store.get_device("dev-1").unwrap().unwrap();
    assert_eq!(fetched.device_id, "dev-1");
    assert_eq!(fetched.owner_id, "owner-1");
    assert_eq!(fetched.status, "active");
}

#[test]
fn get_device_missing() {
    let store = common::test_store();
    let result = store.get_device("nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn put_device_duplicate() {
    let store = common::test_store();
    store.put_device(make_device("dev-1")).unwrap();
    let err = store.put_device(make_device("dev-1")).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("already exists"), "got: {msg}");
}

#[test]
fn list_devices_empty() {
    let store = common::test_store();
    let devices = store.list_devices().unwrap();
    assert!(devices.is_empty());
}

#[test]
fn list_devices_multiple() {
    let store = common::test_store();
    store.put_device(make_device("dev-1")).unwrap();
    store.put_device(make_device("dev-2")).unwrap();
    let devices = store.list_devices().unwrap();
    assert_eq!(devices.len(), 2);
}

#[test]
fn put_and_get_uplinks() {
    let store = common::test_store();
    store
        .put_uplink(make_uplink("dev-1", "2025-01-01T00:00:01Z", 100))
        .unwrap();
    store
        .put_uplink(make_uplink("dev-1", "2025-01-01T00:00:02Z", 200))
        .unwrap();

    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert_eq!(uplinks.len(), 2);
    // Ordered by received_at DESC
    assert_eq!(uplinks[0].received_at, "2025-01-01T00:00:02Z");
    assert_eq!(uplinks[0].payload["current"], 200);
}

#[test]
fn get_uplinks_with_limit() {
    let store = common::test_store();
    for i in 0..5 {
        store
            .put_uplink(make_uplink("dev-1", &format!("2025-01-01T00:00:0{i}Z"), i))
            .unwrap();
    }
    let uplinks = store.get_uplinks("dev-1", 2).unwrap();
    assert_eq!(uplinks.len(), 2);
}

#[test]
fn get_uplinks_empty() {
    let store = common::test_store();
    let uplinks = store.get_uplinks("dev-1", 10).unwrap();
    assert!(uplinks.is_empty());
}

#[test]
fn device_with_subject_dn() {
    let store = common::test_store();
    let mut device = make_device("dev-1");
    device.subject_dn = Some("CN=device1,O=supervictor".into());
    store.put_device(device).unwrap();

    let fetched = store.get_device("dev-1").unwrap().unwrap();
    assert_eq!(
        fetched.subject_dn.as_deref(),
        Some("CN=device1,O=supervictor")
    );
}
