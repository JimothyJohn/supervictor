mod common;

use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};

fn make_device(id: &str) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
        power_threshold_ma: None,
        style: Default::default(),
        desired_fw: None,
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

// ── Per-device power threshold ────────────────────────────────────────

#[test]
fn threshold_round_trips_through_put_get_and_list() {
    let store = common::test_store();
    let mut device = make_device("press-1");
    device.power_threshold_ma = Some(12_000);
    store.put_device(device).unwrap();
    store.put_device(make_device("plain-1")).unwrap();

    let got = store.get_device("press-1").unwrap().unwrap();
    assert_eq!(got.power_threshold_ma, Some(12_000));
    assert_eq!(got.effective_threshold_ma(100), 12_000);
    let plain = store.get_device("plain-1").unwrap().unwrap();
    assert_eq!(plain.power_threshold_ma, None);
    assert_eq!(plain.effective_threshold_ma(100), 100);

    let mut listed = store.list_devices().unwrap();
    listed.sort_by(|a, b| a.device_id.cmp(&b.device_id));
    assert_eq!(
        listed
            .iter()
            .map(|d| d.power_threshold_ma)
            .collect::<Vec<_>>(),
        [None, Some(12_000)]
    );
}

#[test]
fn set_device_threshold_sets_clears_and_rejects_unknown() {
    let store = common::test_store();
    store.put_device(make_device("saw-1")).unwrap();

    let updated = store.set_device_threshold("saw-1", Some(5_000)).unwrap();
    assert_eq!(updated.power_threshold_ma, Some(5_000));
    assert_eq!(updated.status, "active", "other fields untouched");
    assert_eq!(
        store
            .get_device("saw-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(5_000)
    );

    // Extremes are stored as given: the threshold is any i64, like the
    // fleet default.
    for t in [0, -1, i64::MAX, i64::MIN] {
        let updated = store.set_device_threshold("saw-1", Some(t)).unwrap();
        assert_eq!(updated.power_threshold_ma, Some(t), "{t}");
    }

    let cleared = store.set_device_threshold("saw-1", None).unwrap();
    assert_eq!(cleared.power_threshold_ma, None);
    assert_eq!(
        store
            .get_device("saw-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        None
    );

    let err = store.set_device_threshold("ghost", Some(1)).unwrap_err();
    assert!(
        matches!(
            err,
            supervictor_endpoint::error::AppError::DeviceNotFound { .. }
        ),
        "{err:?}"
    );
    assert!(store.get_device("ghost").unwrap().is_none(), "no upsert");
}

/// Forward compatibility: a database created by a binary that predates the
/// column opens cleanly, gains the column, and its old rows read as "no
/// override". Rollback safety: the old column set is untouched, so an old
/// binary's `SELECT device_id, owner_id, subject_dn, status, created_at`
/// keeps working against the migrated file.
#[test]
fn opening_a_pre_threshold_database_migrates_it_in_place() {
    use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
    use supervictor_endpoint::store::DeviceStore;

    let path = std::env::temp_dir().join(format!(
        "supervictor-threshold-migration-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_str = path.to_str().unwrap().to_string();
    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE devices (
                device_id   TEXT PRIMARY KEY,
                owner_id    TEXT NOT NULL,
                subject_dn  TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                created_at  TEXT NOT NULL
            );
            INSERT INTO devices VALUES ('legacy-1', 'owner-1', NULL, 'active', '2025-01-01T00:00:00Z');",
        )
        .unwrap();
    }

    let store = SqliteDeviceStore::new(&path_str).expect("open + migrate");
    let legacy = store.get_device("legacy-1").unwrap().unwrap();
    assert_eq!(legacy.power_threshold_ma, None);
    assert_eq!(legacy.owner_id, "owner-1");
    store.set_device_threshold("legacy-1", Some(9_000)).unwrap();
    drop(store);

    // Reopening is idempotent (the column is not added twice) and the
    // override persisted; an old-shaped query still works.
    let store = SqliteDeviceStore::new(&path_str).expect("reopen");
    assert_eq!(
        store
            .get_device("legacy-1")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(9_000)
    );
    drop(store);
    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        let owner: String = conn
            .query_row(
                "SELECT owner_id FROM devices WHERE device_id = 'legacy-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "owner-1");
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('devices') WHERE name = 'power_threshold_ma'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 1, "column added exactly once");
    }
    let _ = std::fs::remove_file(&path);
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

// ── Per-device chart style ────────────────────────────────────────────

#[test]
fn style_round_trips_and_set_replaces_the_whole_style() {
    use supervictor_common::models::{DeviceStyle, LineStyle, Marker};
    use supervictor_endpoint::error::AppError;
    let store = common::test_store();
    let mut styled = make_device("styled");
    styled.style = DeviceStyle {
        color: Some("#ff8800".into()),
        line_style: Some(LineStyle::DashDot),
        marker: Some(Marker::Triangle),
    };
    store.put_device(styled.clone()).unwrap();
    store.put_device(make_device("plain")).unwrap();

    let got = store.get_device("styled").unwrap().unwrap();
    assert_eq!(got.style, styled.style);
    assert!(store
        .get_device("plain")
        .unwrap()
        .unwrap()
        .style
        .is_default());
    let listed = store.list_devices().unwrap();
    assert_eq!(
        listed.iter().filter(|d| d.style.is_default()).count(),
        1,
        "list carries the style too"
    );

    let updated = store
        .set_device_style(
            "styled",
            DeviceStyle {
                color: Some("s5".into()),
                line_style: None,
                marker: Some(Marker::Square),
            },
        )
        .unwrap();
    assert_eq!(updated.style.color.as_deref(), Some("s5"));
    assert_eq!(updated.style.line_style, None, "omitted part cleared");
    assert_eq!(updated.style.marker, Some(Marker::Square));
    assert_eq!(updated.owner_id, "owner-1", "other columns untouched");
    let cleared = store
        .set_device_style("styled", DeviceStyle::default())
        .unwrap();
    assert!(cleared.style.is_default());

    let err = store
        .set_device_style("ghost", DeviceStyle::default())
        .unwrap_err();
    assert!(matches!(err, AppError::DeviceNotFound { .. }), "{err:?}");
}

/// A database from before the style columns (threshold era) gains them on
/// open; rows read as unstyled; an unknown token written by a newer binary
/// reads as unset rather than failing the row.
#[test]
fn opening_a_pre_style_database_migrates_it_in_place() {
    use supervictor_common::models::{DeviceStyle, LineStyle, Marker};
    use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
    use supervictor_endpoint::store::DeviceStore;

    let path = std::env::temp_dir().join(format!(
        "supervictor-style-migration-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_str = path.to_str().unwrap().to_string();
    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE devices (
                device_id   TEXT PRIMARY KEY,
                owner_id    TEXT NOT NULL,
                subject_dn  TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                created_at  TEXT NOT NULL,
                power_threshold_ma INTEGER
            );
            INSERT INTO devices VALUES ('legacy-1', 'owner-1', NULL, 'active', '2025-01-01T00:00:00Z', 9000);",
        )
        .unwrap();
    }

    let store = SqliteDeviceStore::new(&path_str).expect("open + migrate");
    let legacy = store.get_device("legacy-1").unwrap().unwrap();
    assert!(legacy.style.is_default());
    assert_eq!(legacy.power_threshold_ma, Some(9_000));
    store
        .set_device_style(
            "legacy-1",
            DeviceStyle {
                color: Some("s2".into()),
                line_style: Some(LineStyle::Dotted),
                marker: Some(Marker::Circle),
            },
        )
        .unwrap();
    drop(store);

    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        // A token from the future.
        conn.execute_batch("UPDATE devices SET marker = 'hexagon' WHERE device_id = 'legacy-1'")
            .unwrap();
    }
    let store = SqliteDeviceStore::new(&path_str).expect("reopen");
    let legacy = store.get_device("legacy-1").unwrap().unwrap();
    assert_eq!(legacy.style.color.as_deref(), Some("s2"));
    assert_eq!(legacy.style.line_style, Some(LineStyle::Dotted));
    assert_eq!(legacy.style.marker, None, "unknown token reads as unset");
    drop(store);
    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('devices') WHERE name IN ('color', 'line_style', 'marker')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 3, "each column added exactly once");
    }
    let _ = std::fs::remove_file(&path);
}

// ── desired_fw (OTA target) ────────────────────────────────────────────

#[test]
fn set_desired_fw_sets_clears_and_rejects_unknown() {
    let store = common::test_store();
    store.put_device(make_device("rack-1")).unwrap();
    assert_eq!(
        store.get_device("rack-1").unwrap().unwrap().desired_fw,
        None
    );

    let updated = store.set_desired_fw("rack-1", Some("0.2.0")).unwrap();
    assert_eq!(updated.desired_fw.as_deref(), Some("0.2.0"));
    assert_eq!(updated.status, "active", "other fields untouched");
    assert_eq!(
        store
            .get_device("rack-1")
            .unwrap()
            .unwrap()
            .desired_fw
            .as_deref(),
        Some("0.2.0")
    );
    let listed = store.list_devices().unwrap();
    assert_eq!(listed[0].desired_fw.as_deref(), Some("0.2.0"));

    // The store remembers the string as given (validation is the handler's).
    let updated = store.set_desired_fw("rack-1", Some("1.0.0-rc.1")).unwrap();
    assert_eq!(updated.desired_fw.as_deref(), Some("1.0.0-rc.1"));

    let cleared = store.set_desired_fw("rack-1", None).unwrap();
    assert_eq!(cleared.desired_fw, None);
    assert_eq!(
        store.get_device("rack-1").unwrap().unwrap().desired_fw,
        None
    );

    assert!(matches!(
        store.set_desired_fw("ghost", Some("0.2.0")),
        Err(supervictor_endpoint::error::AppError::DeviceNotFound { .. })
    ));

    // A target given at registration is stored with the row.
    let mut with = make_device("rack-2");
    with.desired_fw = Some("0.3.0".into());
    store.put_device(with).unwrap();
    assert_eq!(
        store
            .get_device("rack-2")
            .unwrap()
            .unwrap()
            .desired_fw
            .as_deref(),
        Some("0.3.0")
    );
}

#[test]
fn opening_a_pre_desired_fw_database_migrates_it_in_place() {
    use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
    use supervictor_endpoint::store::DeviceStore;

    let path = std::env::temp_dir().join(format!(
        "supervictor-desired-fw-migration-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_str = path.to_str().unwrap().to_string();
    {
        // The schema as the style PR left it: every column but desired_fw.
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE devices (
                device_id   TEXT PRIMARY KEY,
                owner_id    TEXT NOT NULL,
                subject_dn  TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                created_at  TEXT NOT NULL,
                power_threshold_ma INTEGER,
                color TEXT,
                line_style TEXT,
                marker TEXT
            );
            INSERT INTO devices VALUES ('legacy-1', 'owner-1', 'CN=legacy-1', 'active', '2025-01-01T00:00:00Z', 9000, 's3', 'dashed', NULL);",
        )
        .unwrap();
    }

    let store = SqliteDeviceStore::new(&path_str).expect("open + migrate");
    let legacy = store.get_device("legacy-1").unwrap().unwrap();
    assert_eq!(legacy.desired_fw, None, "no column = no target");
    assert_eq!(legacy.power_threshold_ma, Some(9_000));
    assert_eq!(legacy.style.color.as_deref(), Some("s3"));
    let updated = store.set_desired_fw("legacy-1", Some("0.2.0")).unwrap();
    assert_eq!(updated.desired_fw.as_deref(), Some("0.2.0"));
    assert_eq!(updated.style.color.as_deref(), Some("s3"), "style survives");

    // Rollback safety: an older binary that never selects the column still
    // reads the row (the column is nullable, no constraint).
    {
        let conn = rusqlite::Connection::open(&path_str).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM devices WHERE device_id = 'legacy-1' AND desired_fw = '0.2.0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
    let _ = std::fs::remove_file(&path);
}
