//! `GET /fleet/history`: admin-gated, whitelist-windowed, bucketed JSON.
//! Seeds through the real ingest route with firmware-shaped payloads so
//! the whole path — parse, persist, window query, bucket — is exercised.

mod common;

use axum::http::{HeaderName, HeaderValue};
use axum_test::TestServer;
use supervictor_endpoint::history::{BUCKETS, METRICS};
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::routes;
use supervictor_endpoint::store::DeviceStore;
use supervictor_endpoint::time::{format_rfc3339, now_rfc3339, parse_rfc3339_unix};

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
        })
        .unwrap();
}

fn now_unix() -> u64 {
    parse_rfc3339_unix(&now_rfc3339()).unwrap()
}

// ── Auth + input ──────────────────────────────────────────────────────

/// The power timeline judges each device against its own threshold: two
/// devices reporting the same 9 A, one on the fleet default (on) and one a
/// press brake with a 12 A bar (off), count as one on + one off in every
/// bucket after their readings.
#[tokio::test]
async fn power_timeline_uses_per_device_thresholds() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "bench-01", "active");
    store
        .put_device(DeviceRecord {
            device_id: "press-01".into(),
            owner_id: "owner-1".into(),
            subject_dn: None,
            status: "active".into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: Some(12_000),
        })
        .unwrap();
    let now = now_unix();
    for id in ["bench-01", "press-01"] {
        store
            .put_uplink(UplinkRecord {
                device_id: id.into(),
                received_at: format_rfc3339(now - 30 * 60, 0),
                payload: serde_json::json!({ "current": 9_000 }),
            })
            .unwrap();
    }

    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/fleet/history?window=1h")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["threshold_ma"], 100, "fleet default still reported");
    let last = body["power"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["on"], 1, "{last}");
    assert_eq!(last["off"], 1, "{last}");
    assert_eq!(last["unknown"], 0, "{last}");
}

#[tokio::test]
async fn history_requires_admin_certificate() {
    let (server, _) = server_with_store();
    let resp = server.get("/fleet/history").await;
    assert_eq!(resp.status_code(), 403, "no cert");
    let (name, value) = dn(DEVICE_DN);
    let resp = server.get("/fleet/history").add_header(name, value).await;
    assert_eq!(resp.status_code(), 403, "device cert");
}

#[tokio::test]
async fn history_rejects_windows_off_the_whitelist() {
    let (server, _) = server_with_store();
    for bad in [
        "2h",
        "1H",
        "60m",
        "1h%0A",
        "1h;DROP",
        "9999999999999d",
        "%00",
    ] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server
            .get(&format!("/fleet/history?window={bad}"))
            .add_header(name, value)
            .await;
        assert_eq!(resp.status_code(), 422, "window={bad}");
        let body: serde_json::Value = resp.json();
        assert_eq!(body["detail"]["field"], "window", "{body}");
    }
}

/// Odd query strings are either ignored (unknown keys) or rejected by the
/// extractor — never a 500.
#[tokio::test]
async fn history_odd_query_strings_never_5xx() {
    let (server, _) = server_with_store();
    for q in [
        "?window=1h&window=6h",
        "?window[]=1h",
        "?%ZZ",
        "?window=",
        "?=1h",
    ] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server
            .get(&format!("/fleet/history{q}"))
            .add_header(name, value)
            .await;
        assert!(
            !resp.status_code().is_server_error(),
            "{q} -> {}",
            resp.status_code()
        );
    }
}

// ── Shape + content ───────────────────────────────────────────────────

#[tokio::test]
async fn history_defaults_to_one_hour_and_buckets_every_device() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "b-dev", "active");
    seed(store.as_ref(), "a-dev", "active");
    seed(store.as_ref(), "revoked", "inactive");

    // Firmware-shaped payload through the real ingest route, twice in the
    // current bucket, so the bucket mean is checkable.
    for current in [100, 300] {
        let resp = server
            .post("/")
            .text(format!(
                r#"{{"id":"a-dev","current":{current},"fw":"0.1.0","seq":1,"uptime_s":5,"interval_s":5,"heap_free":90000,"rssi_dbm":-61,"temp_c":24.5,"humidity_pct":40.0}}"#
            ))
            .await;
        assert_eq!(resp.status_code(), 200, "{}", resp.text());
    }

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/fleet/history").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let h: serde_json::Value = resp.json();
    assert_eq!(h["window"], "1h");
    assert_eq!(h["step_s"], 40);
    assert_eq!(h["truncated"], false);
    assert_eq!(
        parse_rfc3339_unix(h["until"].as_str().unwrap()).unwrap()
            - parse_rfc3339_unix(h["since"].as_str().unwrap()).unwrap(),
        3600
    );

    let devices = h["devices"].as_array().unwrap();
    let ids: Vec<&str> = devices
        .iter()
        .map(|d| d["device_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["a-dev", "b-dev", "revoked"],
        "every registered device, sorted"
    );
    for d in devices {
        assert_eq!(
            d["points"].as_array().unwrap().len(),
            BUCKETS,
            "{}",
            d["device_id"]
        );
    }
    let a = &devices[0];
    assert_eq!(a["uplinks"], 2);
    let last = &a["points"][BUCKETS - 1];
    assert_eq!(last["count"], 2);
    assert_eq!(last["current"], 200.0, "mean of 100 and 300: {last}");
    assert_eq!(last["temp_c"], 24.5);
    assert_eq!(last["rssi_dbm"], -61.0);
    assert_eq!(last["heap_free"], 90000.0);
    for m in METRICS {
        assert!(last.get(m.key).is_some(), "{} missing from point", m.key);
    }
    let silent = &devices[1];
    assert_eq!(silent["uplinks"], 0);
    assert!(silent["points"][0]["current"].is_null());

    assert_eq!(h["threshold_ma"], 100);
    let power = h["power"].as_array().unwrap();
    assert_eq!(power.len(), BUCKETS);
    let p = &power[BUCKETS - 1];
    assert_eq!(p["on"], 1, "a-dev mean 200 mA >= 100: {p}");
    assert_eq!(p["off"], 0, "{p}");
    assert_eq!(p["unknown"], 1, "b-dev never reported: {p}");
    assert_eq!(power[0]["unknown"], 2, "nobody had reported an hour ago");

    let timeline = h["timeline"].as_array().unwrap();
    assert_eq!(timeline.len(), BUCKETS);
    let end = &timeline[BUCKETS - 1];
    assert_eq!(end["fresh"], 1, "{end}");
    assert_eq!(end["never"], 1, "{end}");
    assert_eq!(end["dark"], 0, "{end}");
    assert!(
        end.get("inactive").is_none(),
        "revoked device is not in the timeline"
    );
    let start = &timeline[0];
    assert_eq!(
        start["never"], 2,
        "nobody had uplinked an hour ago: {start}"
    );
}

#[tokio::test]
async fn history_honours_each_window_preset() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "dev", "active");
    // One uplink 20 minutes ago: inside 1h+, outside 15m.
    store
        .put_uplink(UplinkRecord {
            device_id: "dev".into(),
            received_at: format_rfc3339(now_unix() - 20 * 60, 0),
            payload: serde_json::json!({ "current": 5 }),
        })
        .unwrap();
    for (w, seconds, expect_uplinks) in [
        ("15m", 900, 0),
        ("1h", 3600, 1),
        ("6h", 21600, 1),
        ("24h", 86400, 1),
        ("7d", 604800, 1),
    ] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server
            .get(&format!("/fleet/history?window={w}"))
            .add_header(name, value)
            .await;
        assert_eq!(resp.status_code(), 200, "{w}");
        let h: serde_json::Value = resp.json();
        assert_eq!(h["window"], w);
        assert_eq!(h["step_s"], seconds / BUCKETS as u64, "{w}");
        assert_eq!(h["devices"][0]["uplinks"], expect_uplinks, "{w}: {h}");
    }
}

/// A payload with hostile values must not break the aggregate: strings,
/// nulls, arrays and huge numbers are skipped per field, the row still
/// counts.
#[tokio::test]
async fn history_survives_hostile_payload_values() {
    let (server, store) = server_with_store();
    seed(store.as_ref(), "dev", "active");
    store
        .put_uplink(UplinkRecord {
            device_id: "dev".into(),
            received_at: now_rfc3339(),
            payload: serde_json::json!({
                "current": "NaN", "temp_c": null, "humidity_pct": [1,2],
                "rssi_dbm": {"x":1}, "heap_free": 1e308
            }),
        })
        .unwrap();
    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/fleet/history").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let h: serde_json::Value = resp.json();
    let last = &h["devices"][0]["points"][BUCKETS - 1];
    assert_eq!(last["count"], 1);
    assert!(last["current"].is_null());
    assert_eq!(last["heap_free"], 1e308);
    assert_eq!(h["timeline"][BUCKETS - 1]["fresh"], 1);
}
