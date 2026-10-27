//! Store-trait methods added for the fleet dashboard: status mutation and
//! last-uplink aggregation, against real SQLite.

mod common;

use supervictor_endpoint::error::AppError;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};

fn device(id: &str) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2026-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        min_on_s: None,
        style: Default::default(),
        desired_fw: None,
    }
}

fn uplink(id: &str, at: &str) -> UplinkRecord {
    UplinkRecord {
        device_id: id.into(),
        received_at: at.into(),
        payload: serde_json::json!({ "current": 1 }),
    }
}

#[test]
fn set_device_status_updates_and_returns_record() {
    let store = common::test_store();
    store.put_device(device("dev-1")).unwrap();

    let updated = store.set_device_status("dev-1", "inactive").unwrap();
    assert_eq!(updated.status, "inactive");
    assert_eq!(updated.device_id, "dev-1");
    assert_eq!(
        store.get_device("dev-1").unwrap().unwrap().status,
        "inactive"
    );
}

#[test]
fn set_device_status_unknown_device_is_not_found() {
    let store = common::test_store();
    let err = store.set_device_status("ghost", "inactive").unwrap_err();
    assert!(
        matches!(err, AppError::DeviceNotFound { .. }),
        "got {err:?}"
    );
}

#[test]
fn latest_uplinks_returns_max_per_device() {
    let store = common::test_store();
    store.put_device(device("dev-1")).unwrap();
    store.put_device(device("dev-2")).unwrap();
    store.put_device(device("dev-silent")).unwrap();

    store
        .put_uplink(uplink("dev-1", "2026-07-01T10:00:00+00:00"))
        .unwrap();
    store
        .put_uplink(uplink("dev-1", "2026-07-01T12:00:00+00:00"))
        .unwrap();
    store
        .put_uplink(uplink("dev-1", "2026-07-01T11:00:00+00:00"))
        .unwrap();
    store
        .put_uplink(uplink("dev-2", "2026-06-30T00:00:00+00:00"))
        .unwrap();

    let mut latest: Vec<(String, String)> = store
        .latest_uplinks()
        .unwrap()
        .into_iter()
        .map(|u| (u.device_id, u.received_at))
        .collect();
    latest.sort();
    assert_eq!(
        latest,
        vec![
            ("dev-1".to_string(), "2026-07-01T12:00:00+00:00".to_string()),
            ("dev-2".to_string(), "2026-06-30T00:00:00+00:00".to_string()),
        ],
        "max per device, silent devices absent"
    );
}

#[test]
fn latest_uplinks_carries_the_max_rows_payload() {
    let store = common::test_store();
    store.put_device(device("dev-1")).unwrap();
    store
        .put_uplink(UplinkRecord {
            device_id: "dev-1".into(),
            received_at: "2026-07-01T10:00:00+00:00".into(),
            payload: serde_json::json!({ "current": 1, "fw": "0.0.9" }),
        })
        .unwrap();
    store
        .put_uplink(UplinkRecord {
            device_id: "dev-1".into(),
            received_at: "2026-07-01T12:00:00+00:00".into(),
            payload: serde_json::json!({ "current": 2, "fw": "0.1.0" }),
        })
        .unwrap();

    let latest = store.latest_uplinks().unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(
        latest[0].payload["fw"], "0.1.0",
        "payload must come from the newest row, not an arbitrary one"
    );
}

#[test]
fn latest_uplinks_empty_store() {
    let store = common::test_store();
    assert!(store.latest_uplinks().unwrap().is_empty());
}

// ── Window queries for the history views ─────────────────────────────

fn seed_window(store: &dyn supervictor_endpoint::store::DeviceStore) {
    store.put_device(device("dev-1")).unwrap();
    store.put_device(device("dev-2")).unwrap();
    for (id, at) in [
        ("dev-1", "2026-07-01T09:59:59+00:00"),
        ("dev-1", "2026-07-01T10:00:00+00:00"),
        ("dev-2", "2026-07-01T10:00:00.250+00:00"),
        ("dev-1", "2026-07-01T10:05:00+00:00"),
        ("dev-2", "2026-07-01T10:10:00+00:00"),
        ("dev-1", "2026-07-01T10:15:00+00:00"),
    ] {
        store.put_uplink(uplink(id, at)).unwrap();
    }
}

#[test]
fn uplinks_since_is_inclusive_ascending_and_capped_at_the_old_end() {
    let store = common::test_store();
    seed_window(store.as_ref());

    let rows = store
        .uplinks_since("2026-07-01T10:00:00+00:00", 100)
        .unwrap();
    let got: Vec<(String, String)> = rows
        .into_iter()
        .map(|u| (u.received_at, u.device_id))
        .collect();
    assert_eq!(
        got,
        vec![
            ("2026-07-01T10:00:00+00:00".to_string(), "dev-1".to_string()),
            ("2026-07-01T10:00:00.250+00:00".into(), "dev-2".into()),
            ("2026-07-01T10:05:00+00:00".into(), "dev-1".into()),
            ("2026-07-01T10:10:00+00:00".into(), "dev-2".into()),
            ("2026-07-01T10:15:00+00:00".into(), "dev-1".into()),
        ],
        "boundary row included, 09:59:59 excluded, fractional second sorts after whole"
    );

    // The cap keeps the OLDEST rows: a truncated window is visible at the
    // recent end, where the caller flags it.
    let capped = store.uplinks_since("2026-07-01T10:00:00+00:00", 2).unwrap();
    let got: Vec<String> = capped.into_iter().map(|u| u.received_at).collect();
    assert_eq!(
        got,
        vec![
            "2026-07-01T10:00:00+00:00".to_string(),
            "2026-07-01T10:00:00.250+00:00".to_string(),
        ]
    );

    assert!(store
        .uplinks_since("2026-07-01T10:15:00.001+00:00", 100)
        .unwrap()
        .is_empty());
    assert!(store
        .uplinks_since("2099-01-01T00:00:00+00:00", 100)
        .unwrap()
        .is_empty());
    assert!(store
        .uplinks_since("2026-07-01T10:00:00+00:00", 0)
        .unwrap()
        .is_empty());
}

#[test]
fn device_uplinks_since_filters_by_device() {
    let store = common::test_store();
    seed_window(store.as_ref());

    let rows = store
        .device_uplinks_since("dev-2", "2026-07-01T10:00:00+00:00", 100)
        .unwrap();
    let got: Vec<String> = rows.iter().map(|u| u.received_at.clone()).collect();
    assert_eq!(
        got,
        vec![
            "2026-07-01T10:00:00.250+00:00".to_string(),
            "2026-07-01T10:10:00+00:00".to_string(),
        ]
    );
    assert!(rows.iter().all(|u| u.device_id == "dev-2"));

    let capped = store
        .device_uplinks_since("dev-1", "2026-07-01T00:00:00+00:00", 1)
        .unwrap();
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0].received_at, "2026-07-01T09:59:59+00:00");

    assert!(store
        .device_uplinks_since("ghost", "2026-07-01T00:00:00+00:00", 100)
        .unwrap()
        .is_empty());
}

/// `since` is bound as a parameter, never spliced: a hostile string is
/// just a string that matches nothing.
#[test]
fn since_is_a_parameter_not_sql() {
    let store = common::test_store();
    seed_window(store.as_ref());
    for hostile in [
        "' OR 1=1 --",
        "2026-07-01T10:00:00+00:00' OR '1'='1",
        "\u{0}",
        "",
    ] {
        let rows = store.uplinks_since(hostile, 100).unwrap();
        // Compared as a value, whatever it sorts against: no error, never
        // more rows than exist, table intact afterwards.
        assert!(rows.len() <= 6, "{hostile:?} -> {}", rows.len());
        let _ = store.device_uplinks_since(hostile, hostile, 100).unwrap();
    }
    assert_eq!(store.get_uplinks("dev-1", 100).unwrap().len(), 4);
}

/// Payload JSON that is not an object (or not JSON) must come back as
/// `null`, not fail the whole window — one bad row cannot blank a chart.
#[test]
fn window_query_tolerates_unparseable_payload_rows() {
    let store = common::test_store();
    store.put_device(device("dev-1")).unwrap();
    store
        .put_uplink(UplinkRecord {
            device_id: "dev-1".into(),
            received_at: "2026-07-01T10:00:00+00:00".into(),
            payload: serde_json::Value::String("not an object".into()),
        })
        .unwrap();
    store
        .put_uplink(uplink("dev-1", "2026-07-01T10:00:01+00:00"))
        .unwrap();
    let rows = store
        .uplinks_since("2026-07-01T10:00:00+00:00", 100)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].payload["current"], 1);
}
