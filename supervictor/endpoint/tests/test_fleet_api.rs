mod common;

use axum::http::{HeaderName, HeaderValue};
use axum_test::TestServer;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::routes::{self, RouterOptions};
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

fn seed(store: &dyn DeviceStore, id: &str, status: &str) {
    store
        .put_device(DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: None,
            status: status.into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: None,
            style: Default::default(),
        })
        .unwrap();
}

// ── Auth ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn fleet_routes_require_admin_certificate() {
    let (server, _) = server_with_store();
    for path in ["/fleet", "/fleet/summary"] {
        let resp = server.get(path).await;
        assert_eq!(resp.status_code(), 403, "GET {path} without cert");

        let (name, value) = dn(DEVICE_DN);
        let resp = server.get(path).add_header(name, value).await;
        assert_eq!(resp.status_code(), 403, "GET {path} with device cert");
    }
}

// ── Per-device power threshold ────────────────────────────────────────

fn seed_with_threshold(store: &dyn DeviceStore, id: &str, threshold: Option<i64>) {
    store
        .put_device(DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: None,
            status: "active".into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: threshold,
            style: Default::default(),
        })
        .unwrap();
}

fn uplink_now(store: &dyn DeviceStore, id: &str, current: i64) {
    store
        .put_uplink(UplinkRecord {
            device_id: id.into(),
            received_at: now_rfc3339(),
            payload: serde_json::json!({ "current": current }),
        })
        .unwrap();
}

/// The same 9 A reading is "on" for a bench board on the fleet default and
/// "off" for a press brake whose idle pump sits at 9 A.
#[tokio::test]
async fn fleet_judges_power_against_each_devices_threshold() {
    let (server, store) = server_with_store();
    seed_with_threshold(store.as_ref(), "bench-01", None);
    seed_with_threshold(store.as_ref(), "press-01", Some(12_000));
    seed_with_threshold(store.as_ref(), "exact-01", Some(9_000));
    for id in ["bench-01", "press-01", "exact-01"] {
        uplink_now(store.as_ref(), id, 9_000);
    }

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/fleet").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    let by_id = |id: &str| {
        body.as_array()
            .unwrap()
            .iter()
            .find(|d| d["device_id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} missing from {body}"))
    };
    let bench = by_id("bench-01");
    assert_eq!(bench["power"], "on", "{bench}");
    assert_eq!(bench["threshold_ma"], 100);
    assert_eq!(bench["threshold_overridden"], false);
    let press = by_id("press-01");
    assert_eq!(press["power"], "off", "{press}");
    assert_eq!(press["threshold_ma"], 12_000);
    assert_eq!(press["threshold_overridden"], true);
    assert_eq!(by_id("exact-01")["power"], "on", "threshold itself is on");

    let (name, value) = dn(ADMIN_DN);
    let summary: serde_json::Value = server
        .get("/fleet/summary")
        .add_header(name, value)
        .await
        .json();
    assert_eq!(summary["on"], 2, "{summary}");
    assert_eq!(summary["off"], 1, "{summary}");
}

#[tokio::test]
async fn threshold_route_requires_admin_certificate() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01", "active");
    let body = serde_json::json!({ "power_threshold_ma": 5000 });

    let resp = server
        .put("/devices/factory-01/threshold")
        .json(&body)
        .await;
    assert_eq!(resp.status_code(), 403, "no cert");
    let (name, value) = dn(DEVICE_DN);
    let resp = server
        .put("/devices/factory-01/threshold")
        .add_header(name, value)
        .json(&body)
        .await;
    assert_eq!(resp.status_code(), 403, "device cert");
    // A device cannot raise or lower its own bar.
    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        None
    );
}

#[tokio::test]
async fn threshold_route_sets_clears_and_validates() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "factory-01", "active");
    let put = |path: &'static str, body: &'static str| {
        let (name, value) = dn(ADMIN_DN);
        server
            .put(path)
            .add_header(name, value)
            .add_header(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("application/json"),
            )
            .text(body)
    };

    let resp = put(
        "/devices/factory-01/threshold",
        r#"{"power_threshold_ma": 12000}"#,
    )
    .await;
    assert_eq!(resp.status_code(), 200, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    assert_eq!(body["power_threshold_ma"], 12_000);
    assert_eq!(body["device_id"], "factory-01");
    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(12_000)
    );
    // The device document reports it too.
    let (name, value) = dn(ADMIN_DN);
    let device: serde_json::Value = server
        .get("/devices/factory-01")
        .add_header(name, value)
        .await
        .json();
    assert_eq!(device["power_threshold_ma"], 12_000);

    // Every spelling of "clear".
    for clear in [r#"{"power_threshold_ma": null}"#, "{}", "null", "", "   "] {
        put(
            "/devices/factory-01/threshold",
            r#"{"power_threshold_ma": 7}"#,
        )
        .await;
        let resp = put("/devices/factory-01/threshold", clear).await;
        assert_eq!(resp.status_code(), 200, "clear with {clear:?}");
        let body: serde_json::Value = resp.json();
        assert!(
            body.get("power_threshold_ma").is_none(),
            "cleared threshold must be absent, got {body}"
        );
        assert_eq!(
            store
                .get_device("factory-01")
                .unwrap()
                .unwrap()
                .power_threshold_ma,
            None,
            "clear with {clear:?}"
        );
    }

    // Rejected bodies leave the stored value alone.
    put(
        "/devices/factory-01/threshold",
        r#"{"power_threshold_ma": 300}"#,
    )
    .await;
    for bad in [
        r#"{"power_threshold_ma": "12000"}"#,
        r#"{"power_threshold_ma": 1.5}"#,
        r#"{"power_threshold_ma": 1e30}"#,
        r#"{"power_threshold_ma": true}"#,
        r#"{"power_threshold_ma":"#,
        // A misspelt key must not read as "clear".
        r#"{"threshold_ma": 5}"#,
        r#"{"power_threshold_ma": 5, "status": "inactive"}"#,
        "[]",
        "12000",
        r#""12000""#,
    ] {
        let resp = put("/devices/factory-01/threshold", bad).await;
        assert_eq!(resp.status_code(), 422, "body {bad:?}: {}", resp.text());
        assert_eq!(
            store
                .get_device("factory-01")
                .unwrap()
                .unwrap()
                .power_threshold_ma,
            Some(300),
            "body {bad:?} must not change the stored threshold"
        );
    }

    let resp = put("/devices/ghost/threshold", r#"{"power_threshold_ma": 1}"#).await;
    assert_eq!(resp.status_code(), 404, "{}", resp.text());
    assert!(store.get_device("ghost").unwrap().is_none(), "no upsert");
}

#[tokio::test]
async fn registration_accepts_an_initial_threshold() {
    let (server, store) = server_with_store();
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .post("/devices")
        .add_header(name, value)
        .json(&serde_json::json!({
            "device_id": "oven-01",
            "owner_id": "owner-1",
            "power_threshold_ma": 10_000
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "{}", resp.text());
    let body: serde_json::Value = resp.json();
    assert_eq!(body["power_threshold_ma"], 10_000);
    assert_eq!(
        store
            .get_device("oven-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(10_000)
    );

    // Old clients that don't send the field still register, with no override.
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .post("/devices")
        .add_header(name, value)
        .json(&serde_json::json!({ "device_id": "plain-01", "owner_id": "owner-1" }))
        .await;
    assert_eq!(resp.status_code(), 201);
    let body: serde_json::Value = resp.json();
    assert!(body.get("power_threshold_ma").is_none(), "{body}");
}

// ── /fleet ────────────────────────────────────────────────────────────

#[tokio::test]
async fn fleet_reports_staleness_and_fw() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "fresh-dev", "active");
    seed(store.as_ref(), "silent-dev", "active");
    seed(store.as_ref(), "revoked-dev", "inactive");

    // A just-now uplink carrying a firmware version.
    store
        .put_uplink(UplinkRecord {
            device_id: "fresh-dev".into(),
            received_at: now_rfc3339(),
            payload: serde_json::json!({ "current": 5, "fw": "0.1.0" }),
        })
        .unwrap();

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/fleet").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let fleet: serde_json::Value = resp.json();
    let devices = fleet.as_array().unwrap();
    assert_eq!(devices.len(), 3);

    let by_id = |id: &str| {
        devices
            .iter()
            .find(|d| d["device_id"] == id)
            .unwrap_or_else(|| panic!("{id} missing from fleet"))
    };
    assert_eq!(by_id("fresh-dev")["staleness"], "fresh");
    assert_eq!(by_id("fresh-dev")["fw"], "0.1.0");
    assert_eq!(by_id("fresh-dev")["current"], 5);
    assert_eq!(
        by_id("fresh-dev")["power"],
        "off",
        "5 mA is under the 100 mA default"
    );
    assert_eq!(by_id("silent-dev")["staleness"], "unknown");
    assert_eq!(by_id("silent-dev")["power"], "unknown");
    assert!(by_id("silent-dev").get("current").is_none());
    assert!(
        by_id("silent-dev").get("fw").is_none(),
        "no fw when never uplinked"
    );
    assert_eq!(by_id("revoked-dev")["status"], "inactive");
}

// ── /fleet/summary ────────────────────────────────────────────────────

#[tokio::test]
async fn summary_counts_only_active_devices_in_staleness_buckets() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "fresh-dev", "active");
    seed(store.as_ref(), "never-dev", "active");
    seed(store.as_ref(), "revoked-dev", "inactive");
    // Revoked device also uplinked long ago — must not count as dark.
    store
        .put_uplink(UplinkRecord {
            device_id: "revoked-dev".into(),
            received_at: "2026-01-01T00:00:00+00:00".into(),
            payload: serde_json::json!({ "current": 1 }),
        })
        .unwrap();
    store
        .put_uplink(UplinkRecord {
            device_id: "fresh-dev".into(),
            received_at: now_rfc3339(),
            payload: serde_json::json!({ "current": 2 }),
        })
        .unwrap();

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/fleet/summary").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let summary: serde_json::Value = resp.json();
    assert_eq!(summary["total"], 3);
    assert_eq!(summary["active"], 2);
    assert_eq!(summary["inactive"], 1);
    assert_eq!(summary["fresh"], 1);
    assert_eq!(summary["never"], 1);
    assert_eq!(summary["dark"], 0, "inactive devices don't alarm");
    assert_eq!(summary["off"], 1, "fresh-dev at 2 mA");
    assert_eq!(summary["on"], 0);
}

// ── Power state ───────────────────────────────────────────────────────

/// Power is judged against the configured threshold: at or above is on,
/// below is off, a dark device is unknown even with a reading, and the
/// summary counts only active devices.
#[tokio::test]
async fn power_state_follows_the_configured_threshold() {
    let store = common::test_store();
    let app = routes::router_with(
        store.clone(),
        RouterOptions {
            require_registration: false,
            power_threshold_ma: 500,
        },
    );
    let server = TestServer::new(app);
    seed(store.as_ref(), "pump", "active");
    seed(store.as_ref(), "idle", "active");
    seed(store.as_ref(), "gone", "active");
    seed(store.as_ref(), "revoked", "inactive");
    for (id, at, current) in [
        ("pump", now_rfc3339(), 500),
        ("idle", now_rfc3339(), 499),
        ("gone", "2026-01-01T00:00:00+00:00".to_string(), 9000),
        ("revoked", now_rfc3339(), 9000),
    ] {
        store
            .put_uplink(UplinkRecord {
                device_id: id.into(),
                received_at: at,
                payload: serde_json::json!({ "current": current }),
            })
            .unwrap();
    }

    let (name, value) = dn(ADMIN_DN);
    let fleet: serde_json::Value = server.get("/fleet").add_header(name, value).await.json();
    let power = |id: &str| {
        fleet
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["device_id"] == id)
            .unwrap()["power"]
            .clone()
    };
    assert_eq!(power("pump"), "on", "threshold itself is on");
    assert_eq!(power("idle"), "off");
    assert_eq!(
        power("gone"),
        "unknown",
        "a dark device's reading is not trusted"
    );
    assert_eq!(power("revoked"), "on", "per-device state is still reported");

    let (name, value) = dn(ADMIN_DN);
    let summary: serde_json::Value = server
        .get("/fleet/summary")
        .add_header(name, value)
        .await
        .json();
    assert_eq!(summary["on"], 1, "{summary}");
    assert_eq!(summary["off"], 1, "{summary}");
    assert_eq!(summary["active"], 3);
}

// ── End-to-end: device uplink with fw surfaces in the fleet view ─────

#[tokio::test]
async fn uplink_fw_flows_through_to_fleet() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "e2e-dev", "active");

    // Post through the real ingest route, exactly as firmware would.
    let resp = server
        .post("/")
        .text(r#"{"id":"e2e-dev","current":7,"fw":"9.9.9"}"#)
        .await;
    assert_eq!(resp.status_code(), 200);

    let (name, value) = dn(ADMIN_DN);
    let fleet: serde_json::Value = server.get("/fleet").add_header(name, value).await.json();
    assert_eq!(fleet[0]["fw"], "9.9.9");
    assert_eq!(fleet[0]["staleness"], "fresh");
}

/// Pre-fw firmware (no fw field) still ingests cleanly — rollout safety.
#[tokio::test]
async fn uplink_without_fw_still_accepted() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "old-dev", "active");
    let resp = server
        .post("/")
        .text(r#"{"id":"old-dev","current":3}"#)
        .await;
    assert_eq!(resp.status_code(), 200);

    let (name, value) = dn(ADMIN_DN);
    let fleet: serde_json::Value = server.get("/fleet").add_header(name, value).await.json();
    assert!(fleet[0].get("fw").is_none());
}
