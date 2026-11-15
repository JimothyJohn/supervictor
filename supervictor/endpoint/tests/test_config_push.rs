//! Pushed device settings (decided 2026-11-02): `PUT /devices/{id}/config`
//! sets the uplink interval; every registered device's uplink response
//! carries a `config` block (version, interval, effective threshold); the
//! device echoes `config_version` and the fleet reports `config_pending`
//! until the echo matches.

mod common;

use axum::http::{HeaderName, HeaderValue};
use axum_test::TestServer;
use supervictor_common::config;
use supervictor_endpoint::error::AppError;
use supervictor_endpoint::fleet::DEFAULT_POWER_THRESHOLD_MA;
use supervictor_endpoint::handlers;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::routes;
use supervictor_endpoint::store::DeviceStore;
use supervictor_endpoint::time::now_rfc3339;

const SUBJECT_HEADER: &str = "x-ssl-client-subject-dn";
const ADMIN_DN: &str = "CN=ops,OU=admin,O=supervictor";
const DEVICE_DN: &str = "CN=factory-01,OU=devices,O=supervictor";

fn server_with_store() -> (TestServer, std::sync::Arc<dyn DeviceStore>) {
    let store = common::test_store();
    let app = routes::router(store.clone());
    (TestServer::new(app), store)
}

fn dn(value: &'static str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(SUBJECT_HEADER),
        HeaderValue::from_static(value),
    )
}

fn seed(store: &dyn DeviceStore, id: &str) {
    store
        .put_device(DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: None,
            status: "active".into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: None,
            min_on_s: None,
            style: Default::default(),
            desired_fw: None,
            uplink_interval_s: None,
        })
        .unwrap();
}

fn put_json(server: &TestServer, path: &str, body: &str) -> axum_test::TestRequest {
    let (name, value) = dn(ADMIN_DN);
    server
        .put(path)
        .add_header(name, value)
        .add_header(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        )
        .text(body.to_string())
}

fn stored_interval(store: &dyn DeviceStore, id: &str) -> Option<u32> {
    store.get_device(id).unwrap().unwrap().uplink_interval_s
}

// ── Route ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn config_route_requires_the_admin_certificate() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01");
    let resp = server
        .put("/devices/factory-01/config")
        .text(r#"{"uplink_interval_s": 10}"#)
        .await;
    assert_eq!(resp.status_code(), 403, "no cert");
    let (name, value) = dn(DEVICE_DN);
    let resp = server
        .put("/devices/factory-01/config")
        .add_header(name, value)
        .text(r#"{"uplink_interval_s": 10}"#)
        .await;
    assert_eq!(resp.status_code(), 403, "device cert");
    assert_eq!(stored_interval(store.as_ref(), "factory-01"), None);
}

#[tokio::test]
async fn config_route_sets_clears_and_validates() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01");

    let resp = put_json(
        &server,
        "/devices/factory-01/config",
        r#"{"uplink_interval_s": 10}"#,
    )
    .await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    assert_eq!(body["uplink_interval_s"], 10);
    assert_eq!(stored_interval(store.as_ref(), "factory-01"), Some(10));
    let (name, value) = dn(ADMIN_DN);
    let device: serde_json::Value = server
        .get("/devices/factory-01")
        .add_header(name, value)
        .await
        .json();
    assert_eq!(device["uplink_interval_s"], 10);

    // The bounds are inclusive.
    for ok in [config::UPLINK_INTERVAL_MIN_S, config::UPLINK_INTERVAL_MAX_S] {
        let resp = put_json(
            &server,
            "/devices/factory-01/config",
            &format!(r#"{{"uplink_interval_s": {ok}}}"#),
        )
        .await;
        assert_eq!(resp.status_code(), 200, "{ok}: {}", resp.text());
        assert_eq!(stored_interval(store.as_ref(), "factory-01"), Some(ok));
    }

    // Every spelling of "back to the firmware default".
    for clear in [r#"{"uplink_interval_s": null}"#, "{}", "null", "", "   "] {
        put_json(
            &server,
            "/devices/factory-01/config",
            r#"{"uplink_interval_s": 7}"#,
        )
        .await;
        let resp = put_json(&server, "/devices/factory-01/config", clear).await;
        assert_eq!(resp.status_code(), 200, "clear with {clear:?}");
        let body: serde_json::Value = resp.json();
        assert!(body.get("uplink_interval_s").is_none(), "{body}");
        assert_eq!(
            stored_interval(store.as_ref(), "factory-01"),
            None,
            "{clear:?}"
        );
    }

    // Rejected bodies leave the stored value alone.
    put_json(
        &server,
        "/devices/factory-01/config",
        r#"{"uplink_interval_s": 30}"#,
    )
    .await;
    for bad in [
        r#"{"uplink_interval_s": 0}"#,
        r#"{"uplink_interval_s": 601}"#,
        r#"{"uplink_interval_s": -5}"#,
        r#"{"uplink_interval_s": 4294967296}"#,
        r#"{"uplink_interval_s": "10"}"#,
        r#"{"uplink_interval_s": 1.5}"#,
        r#"{"uplink_interval_s": true}"#,
        r#"{"uplink_interval_s":"#,
        // A misspelt or extra key must not read as "clear".
        r#"{"interval_s": 10}"#,
        r#"{"uplink_interval_s": 10, "power_threshold_ma": 5}"#,
        "[]",
        "10",
        r#""10""#,
    ] {
        let resp = put_json(&server, "/devices/factory-01/config", bad).await;
        assert_eq!(resp.status_code(), 422, "body {bad:?}: {}", resp.text());
        assert_eq!(
            stored_interval(store.as_ref(), "factory-01"),
            Some(30),
            "body {bad:?} must not change the stored interval"
        );
    }

    let resp = put_json(
        &server,
        "/devices/ghost/config",
        r#"{"uplink_interval_s": 1}"#,
    )
    .await;
    assert_eq!(resp.status_code(), 404);
    assert!(store.get_device("ghost").unwrap().is_none(), "no upsert");
    let resp = put_json(
        &server,
        "/devices/bad%00id/config",
        r#"{"uplink_interval_s": 1}"#,
    )
    .await;
    assert!(
        resp.status_code() == 422 || resp.status_code() == 400,
        "{}",
        resp.status_code()
    );
}

// ── Uplink response ───────────────────────────────────────────────────

#[tokio::test]
async fn every_registered_uplink_gets_the_config_block() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01");

    // Fresh registration: firmware-default interval, fleet threshold.
    let resp = server
        .post("/")
        .json(&serde_json::json!({"id": "factory-01", "current": 5}))
        .await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    let expect = config::version(0, DEFAULT_POWER_THRESHOLD_MA);
    assert_eq!(
        body["config"],
        serde_json::json!({
            "version": expect,
            "uplink_interval_s": 0,
            "threshold_ma": DEFAULT_POWER_THRESHOLD_MA
        }),
        "{body}"
    );
    assert_ne!(expect, 0);

    // The block follows both settings, through either route.
    put_json(
        &server,
        "/devices/factory-01/config",
        r#"{"uplink_interval_s": 10}"#,
    )
    .await;
    put_json(
        &server,
        "/devices/factory-01/threshold",
        r#"{"power_threshold_ma": 12000}"#,
    )
    .await;
    let resp = server
        .post("/")
        .json(&serde_json::json!({"id": "factory-01", "current": 5}))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["config"]["version"], config::version(10, 12_000));
    assert_eq!(body["config"]["uplink_interval_s"], 10);
    assert_eq!(body["config"]["threshold_ma"], 12_000);

    // An unregistered device (open ingest) has no record to push.
    let resp = server
        .post("/")
        .json(&serde_json::json!({"id": "stranger", "current": 1}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert!(body.get("config").is_none(), "{body}");
}

async fn fleet_row(server: &TestServer) -> serde_json::Value {
    let (an, av) = dn(ADMIN_DN);
    let fleet: Vec<serde_json::Value> = server.get("/fleet").add_header(an, av).await.json();
    fleet
        .into_iter()
        .find(|d| d["device_id"] == "factory-01")
        .unwrap()
}

#[tokio::test]
async fn fleet_reports_pending_until_the_device_echoes_the_version() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01");

    // Never uplinked: pending (nothing has been echoed).
    let row = fleet_row(&server).await;
    let v0 = config::version(0, DEFAULT_POWER_THRESHOLD_MA);
    assert_eq!(row["config_version"], v0);
    assert_eq!(row["config_pending"], true);
    assert!(row.get("uplink_interval_s").is_none());

    // Old firmware: uplinks without the echo stay pending.
    store
        .put_uplink(UplinkRecord {
            device_id: "factory-01".into(),
            received_at: now_rfc3339(),
            payload: serde_json::json!({ "current": 1200 }),
        })
        .unwrap();
    assert_eq!(fleet_row(&server).await["config_pending"], true);

    // The device echoes the current version: applied.
    let resp = server
        .post("/")
        .json(&serde_json::json!({"id": "factory-01", "current": 1200, "config_version": v0}))
        .await;
    assert_eq!(resp.status_code(), 200);
    assert_eq!(fleet_row(&server).await["config_pending"], false);

    // A change on either side of the block makes it pending again…
    put_json(
        &server,
        "/devices/factory-01/config",
        r#"{"uplink_interval_s": 60}"#,
    )
    .await;
    let row = fleet_row(&server).await;
    let v1 = config::version(60, DEFAULT_POWER_THRESHOLD_MA);
    assert_eq!(row["config_version"], v1);
    assert_eq!(row["config_pending"], true);
    assert_eq!(row["uplink_interval_s"], 60);
    // …a stale echo does not clear it…
    server
        .post("/")
        .json(&serde_json::json!({"id": "factory-01", "current": 1200, "config_version": v0}))
        .await;
    assert_eq!(fleet_row(&server).await["config_pending"], true);
    // …the matching one does.
    server
        .post("/")
        .json(&serde_json::json!({"id": "factory-01", "current": 1200, "config_version": v1}))
        .await;
    assert_eq!(fleet_row(&server).await["config_pending"], false);

    // The device page says so too.
    let (an, av) = dn(ADMIN_DN);
    let html = server
        .get("/ui/devices/factory-01")
        .add_header(an, av)
        .await
        .text();
    assert!(html.contains("config-applied"), "{html}");
    assert!(html.contains(&format!("v{v1} on the device")), "{html}");
    put_json(
        &server,
        "/devices/factory-01/threshold",
        r#"{"power_threshold_ma": 5000}"#,
    )
    .await;
    let (an, av) = dn(ADMIN_DN);
    let html = server
        .get("/ui/devices/factory-01")
        .add_header(an, av)
        .await
        .text();
    assert!(html.contains("config-pending"), "{html}");
    assert!(html.contains(r#"name="uplink_interval_s""#), "{html}");
    assert!(html.contains(r#"value="60""#), "{html}");
}

// ── Handler body discipline ───────────────────────────────────────────

#[test]
fn set_config_handler_parses_every_body_shape_and_refuses_out_of_range() {
    let store = common::test_store();
    seed(store.as_ref(), "dev-1");
    let resp = handlers::handle_set_device_config(
        "dev-1",
        Some(r#"{"uplink_interval_s": 300}"#),
        store.as_ref(),
    )
    .unwrap();
    assert_eq!(resp.uplink_interval_s, Some(300));
    for clear in [None, Some(""), Some("  \n"), Some("null"), Some("{}")] {
        handlers::handle_set_device_config(
            "dev-1",
            Some(r#"{"uplink_interval_s": 2}"#),
            store.as_ref(),
        )
        .unwrap();
        let resp = handlers::handle_set_device_config("dev-1", clear, store.as_ref())
            .unwrap_or_else(|e| panic!("{clear:?}: {e}"));
        assert_eq!(resp.uplink_interval_s, None, "{clear:?}");
    }
    for bad in [
        r#"{"uplink_interval_s": 0}"#,
        r#"{"uplink_interval_s": 601}"#,
        r#"{"uplink_interval_s": 10, "x": 1}"#,
        "[]",
    ] {
        let err =
            handlers::handle_set_device_config("dev-1", Some(bad), store.as_ref()).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidPayload { .. }),
            "{bad}: {err:?}"
        );
    }
    assert!(matches!(
        handlers::handle_set_device_config(
            "ghost",
            Some(r#"{"uplink_interval_s": 5}"#),
            store.as_ref()
        ),
        Err(AppError::DeviceNotFound { .. })
    ));
}

// ── Store ─────────────────────────────────────────────────────────────

#[test]
fn sqlite_set_uplink_interval_sets_clears_and_rejects_unknown() {
    let store = common::test_store();
    seed(store.as_ref(), "saw-1");
    let updated = store.set_uplink_interval("saw-1", Some(45)).unwrap();
    assert_eq!(updated.uplink_interval_s, Some(45));
    assert_eq!(updated.status, "active", "other fields untouched");
    assert_eq!(stored_interval(store.as_ref(), "saw-1"), Some(45));
    assert_eq!(
        store.list_devices().unwrap()[0].uplink_interval_s,
        Some(45),
        "listing reads the column too"
    );
    let cleared = store.set_uplink_interval("saw-1", None).unwrap();
    assert_eq!(cleared.uplink_interval_s, None);
    assert_eq!(stored_interval(store.as_ref(), "saw-1"), None);
    assert!(matches!(
        store.set_uplink_interval("ghost", Some(1)),
        Err(AppError::DeviceNotFound { .. })
    ));
    assert!(store.get_device("ghost").unwrap().is_none(), "no upsert");

    // Registration with the field set stores it.
    let mut with = DeviceRecord {
        device_id: "saw-2".into(),
        owner_id: "owner-1".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2026-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        min_on_s: None,
        style: Default::default(),
        desired_fw: None,
        uplink_interval_s: None,
    };
    with.uplink_interval_s = Some(600);
    store.put_device(with).unwrap();
    assert_eq!(stored_interval(store.as_ref(), "saw-2"), Some(600));
}

/// A database from a binary that predates pushed settings opens, gains
/// the column, and its rows read as "firmware default"; the old column
/// set is untouched, so rolling that binary back still works.
#[test]
fn opening_a_pre_config_database_migrates_it_in_place() {
    use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
    let path = std::env::temp_dir().join(format!(
        "supervictor-config-migration-{}-{}.db",
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
                power_threshold_ma INTEGER,
                min_on_s INTEGER,
                color TEXT, line_style TEXT, marker TEXT,
                desired_fw TEXT
            );
            INSERT INTO devices (device_id, owner_id, status, created_at) VALUES ('legacy-1', 'owner-1', 'active', '2025-01-01T00:00:00Z');",
        )
        .unwrap();
    }
    let store = SqliteDeviceStore::new(&path_str).expect("open + migrate");
    let legacy = store.get_device("legacy-1").unwrap().unwrap();
    assert_eq!(legacy.uplink_interval_s, None);
    assert_eq!(
        legacy.config(DEFAULT_POWER_THRESHOLD_MA).uplink_interval_s,
        0,
        "no column yesterday = firmware default today"
    );
    store.set_uplink_interval("legacy-1", Some(15)).unwrap();
    drop(store);
    let store = SqliteDeviceStore::new(&path_str).expect("reopen");
    assert_eq!(stored_interval(&store, "legacy-1"), Some(15));
    drop(store);
    // Rollback safety: the old column set still selects.
    let conn = rusqlite::Connection::open(&path_str).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM devices WHERE device_id = 'legacy-1' AND desired_fw IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    let _ = std::fs::remove_file(&path);
}

/// The version is a pure function of the block, so both sides agree
/// without a counter: equal settings give equal versions, any change
/// gives another one, and it is never the device's "none" (0).
#[test]
fn device_config_version_tracks_the_values() {
    let mut d = DeviceRecord {
        device_id: "d".into(),
        owner_id: "o".into(),
        subject_dn: None,
        status: "active".into(),
        created_at: "2026-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        min_on_s: None,
        style: Default::default(),
        desired_fw: None,
        uplink_interval_s: None,
    };
    let base = d.config(1000);
    assert_eq!((base.uplink_interval_s, base.threshold_ma), (0, 1000));
    assert_eq!(base.version, config::version(0, 1000));
    assert_eq!(d.config(1000), base, "stable");
    assert_ne!(d.config(2000).version, base.version, "fleet default counts");
    d.power_threshold_ma = Some(1000);
    assert_eq!(
        d.config(2000).version,
        base.version,
        "an override equal to the old default is the same block"
    );
    d.uplink_interval_s = Some(5);
    assert_ne!(d.config(1000).version, base.version);
    assert_ne!(d.config(1000).version, 0);
}
