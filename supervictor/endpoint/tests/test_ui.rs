mod common;

use axum::http::{HeaderName, HeaderValue};
use axum_test::TestServer;
use std::sync::Arc;
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::routes;
use supervictor_endpoint::store::DeviceStore;

const SUBJECT_HEADER: &str = "x-ssl-client-subject-dn";
const ADMIN_DN: &str = "CN=nick,OU=admin,O=supervictor";
const DEVICE_DN: &str = "CN=factory-01,OU=devices,O=supervictor";

fn server_with_store() -> (TestServer, Arc<dyn DeviceStore>) {
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

fn seed_device(store: &dyn DeviceStore, id: &str, status: &str) {
    store
        .put_device(DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: Some(format!("CN={id},OU=devices")),
            status: status.into(),
            created_at: "2026-07-01T00:00:00+00:00".into(),
            power_threshold_ma: None,
            min_on_s: None,
            style: Default::default(),
            desired_fw: None,
        })
        .unwrap();
}

/// The desktop layer of a page: everything from the first `.deep` block
/// on. The phone glance that precedes it repeats keys, badges, legends and
/// one chart, so counts of those are taken here.
fn deep(html: &str) -> &str {
    let at = html.find("<div class=\"deep\">").expect("deep layer");
    &html[at..]
}

fn seed_uplink(store: &dyn DeviceStore, id: &str, at: &str, current: i32) {
    store
        .put_uplink(UplinkRecord {
            device_id: id.into(),
            received_at: at.into(),
            payload: serde_json::json!({ "current": current }),
        })
        .unwrap();
}

// ── Auth gate ─────────────────────────────────────────────────────────

#[tokio::test]
async fn every_ui_route_requires_a_certificate() {
    let (server, _) = server_with_store();
    for path in [
        "/ui",
        "/ui/history",
        "/ui/history?window=6h",
        "/ui/live",
        "/ui/live.json",
        "/ui/devices/some-device",
        "/ui/devices/some-device?window=6h",
        "/ui/events",
        "/ui/assets/style.css",
    ] {
        let resp = server.get(path).await;
        assert_eq!(resp.status_code(), 403, "GET {path} without cert");
    }
    let resp = server.post("/ui/devices").await;
    assert_eq!(resp.status_code(), 403, "POST /ui/devices without cert");
    let resp = server.post("/ui/devices/x/status").await;
    assert_eq!(resp.status_code(), 403, "POST status without cert");
}

#[tokio::test]
async fn device_certificate_is_not_admin() {
    let (server, _) = server_with_store();
    let (name, value) = dn(DEVICE_DN);
    let resp = server.get("/ui").add_header(name, value).await;
    assert_eq!(resp.status_code(), 403);
}

#[tokio::test]
async fn admin_ou_must_be_a_dn_component_not_a_substring() {
    let (server, _) = server_with_store();
    for spoof in [
        "CN=OU=admin,O=x",       // OU=admin inside CN value
        "CN=evil,OU=admins,O=x", // prefix-similar OU
        "OU=administrator,O=x",  // longer value
        "CN=admin,O=x",          // admin as CN, no OU
    ] {
        let resp = server
            .get("/ui")
            .add_header(
                HeaderName::from_static(SUBJECT_HEADER),
                HeaderValue::from_str(spoof).unwrap(),
            )
            .await;
        assert_eq!(
            resp.status_code(),
            403,
            "spoof DN should be rejected: {spoof}"
        );
    }
}

#[tokio::test]
async fn admin_ou_is_case_insensitive() {
    let (server, _) = server_with_store();
    let resp = server
        .get("/ui")
        .add_header(
            HeaderName::from_static(SUBJECT_HEADER),
            HeaderValue::from_static("CN=nick, ou=ADMIN, O=supervictor"),
        )
        .await;
    assert_eq!(resp.status_code(), 200);
}

// ── Fleet page ────────────────────────────────────────────────────────

#[tokio::test]
async fn fleet_page_lists_devices_with_status() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    seed_device(store.as_ref(), "factory-02", "inactive");
    seed_uplink(
        store.as_ref(),
        "factory-01",
        "2026-07-01T10:00:00+00:00",
        42,
    );

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(html.contains("factory-01"));
    assert!(html.contains("factory-02"));
    assert!(html.contains("2026-07-01T10:00:00+00:00"));
    assert!(html.contains("register device"));
    // factory-02 never uplinked
    assert!(html.contains("never"));
    // Power column: factory-01's last current (42 mA, 2026) is too old to
    // trust -> unknown; factory-02 has no reading -> unknown.
    assert!(html.contains("<th>power</th>"));
    assert_eq!(
        deep(&html).matches("badge power unknown").count(),
        2,
        "{html}"
    );
    assert!(
        html.contains("42 mA"),
        "last current shown beside the badge"
    );
}

#[tokio::test]
async fn fleet_page_shows_on_and_off_from_fresh_readings() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "pump", "active");
    seed_device(store.as_ref(), "idle", "active");
    let now = supervictor_endpoint::time::now_rfc3339();
    seed_uplink(store.as_ref(), "pump", &now, 1500);
    seed_uplink(store.as_ref(), "idle", &now, 0);
    let (name, value) = dn(ADMIN_DN);
    let html = server.get("/ui").add_header(name, value).await.text();
    assert_eq!(deep(&html).matches("badge power on").count(), 1, "{html}");
    assert_eq!(deep(&html).matches("badge power off").count(), 1, "{html}");
}

#[tokio::test]
async fn fleet_page_sets_csrf_cookie() {
    let (server, _) = server_with_store();
    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui").add_header(name, value).await;
    let cookie = resp
        .headers()
        .get("set-cookie")
        .expect("csrf cookie must be set")
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("sv_csrf="));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("HttpOnly"));
}

#[tokio::test]
async fn hostile_device_id_renders_inert() {
    let (server, store) = server_with_store();
    let hostile = "<script>alert(1)</script>";
    seed_device(store.as_ref(), hostile, "active");

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui").add_header(name, value).await;
    let html = resp.text();
    assert!(
        !html.contains("<script>alert(1)"),
        "raw script tag must not survive templating"
    );
    assert!(html.contains("&lt;script&gt;"), "id should render escaped");
}

// ── Device detail ─────────────────────────────────────────────────────

#[tokio::test]
async fn device_detail_shows_uplinks_and_sparkline() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    for (i, current) in [10, 20, 15, 30].iter().enumerate() {
        seed_uplink(
            store.as_ref(),
            "factory-01",
            &format!("2026-07-01T10:0{i}:00+00:00"),
            *current,
        );
    }

    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/ui/devices/factory-01")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(html.contains("CN=factory-01,OU=devices"));
    assert!(
        html.contains("<figure class=\"chart\""),
        "metric charts should render"
    );
    // maud escapes the JSON's quotes in the payload cell
    assert!(html.contains("&quot;current&quot;:30"));
}

#[tokio::test]
async fn unknown_device_is_404() {
    let (server, _) = server_with_store();
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/ui/devices/ghost")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 404);
}

// ── CSRF + fleet actions ──────────────────────────────────────────────

fn admin_with_cookie(token: &str) -> Vec<(HeaderName, HeaderValue)> {
    vec![
        dn(ADMIN_DN),
        (
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(&format!("sv_csrf={token}")).unwrap(),
        ),
    ]
}

#[tokio::test]
async fn register_without_csrf_cookie_is_403() {
    let (server, store) = server_with_store();
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .post("/ui/devices")
        .add_header(name, value)
        .form(&[
            ("csrf", "whatever"),
            ("device_id", "new-dev"),
            ("owner_id", "owner-9"),
        ])
        .await;
    assert_eq!(resp.status_code(), 403);
    assert!(store.get_device("new-dev").unwrap().is_none());
}

#[tokio::test]
async fn register_with_mismatched_csrf_is_403() {
    let (server, store) = server_with_store();
    let mut req = server.post("/ui/devices");
    for (name, value) in admin_with_cookie("token-a") {
        req = req.add_header(name, value);
    }
    let resp = req
        .form(&[
            ("csrf", "token-b"),
            ("device_id", "new-dev"),
            ("owner_id", "owner-9"),
        ])
        .await;
    assert_eq!(resp.status_code(), 403);
    assert!(store.get_device("new-dev").unwrap().is_none());
}

#[tokio::test]
async fn register_with_valid_csrf_creates_device() {
    let (server, store) = server_with_store();
    let mut req = server.post("/ui/devices");
    for (name, value) in admin_with_cookie("token-a") {
        req = req.add_header(name, value);
    }
    let resp = req
        .form(&[
            ("csrf", "token-a"),
            ("device_id", "new-dev"),
            ("owner_id", "owner-9"),
        ])
        .await;
    assert_eq!(resp.status_code(), 303, "expected redirect after register");
    let device = store.get_device("new-dev").unwrap().expect("device stored");
    assert_eq!(device.status, "active");
}

#[tokio::test]
async fn status_toggle_revokes_and_reactivates() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");

    let mut req = server.post("/ui/devices/factory-01/status");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("status", "inactive")]).await;
    assert_eq!(resp.status_code(), 303);
    assert_eq!(
        store.get_device("factory-01").unwrap().unwrap().status,
        "inactive"
    );

    let mut req = server.post("/ui/devices/factory-01/status");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("status", "active")]).await;
    assert_eq!(resp.status_code(), 303);
    assert_eq!(
        store.get_device("factory-01").unwrap().unwrap().status,
        "active"
    );
}

// ── Per-device power threshold ────────────────────────────────────────

#[tokio::test]
async fn threshold_form_sets_and_clears_the_override() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");

    let mut req = server.post("/ui/devices/factory-01/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req
        .form(&[("csrf", "tok"), ("threshold_ma", " 12000 ")])
        .await;
    assert_eq!(resp.status_code(), 303, "{}", resp.text());
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/ui/devices/factory-01",
        "back to the device page, not the fleet table"
    );
    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(12_000)
    );

    // The device page shows the override and the fleet default side by side;
    // the fleet table marks the row.
    let (name, value) = dn(ADMIN_DN);
    let html = server
        .get("/ui/devices/factory-01")
        .add_header(name, value)
        .await
        .text();
    assert!(html.contains("power threshold"), "{html}");
    assert!(html.contains(r#"value="12000""#), "{html}");
    assert!(html.contains("fleet default 1000 mA"), "{html}");
    assert!(html.contains(r#"placeholder="1000""#), "{html}");
    let (name, value) = dn(ADMIN_DN);
    let fleet_html = server.get("/ui").add_header(name, value).await.text();
    assert!(fleet_html.contains("≥ 12000"), "{fleet_html}");

    // Empty submission clears.
    let mut req = server.post("/ui/devices/factory-01/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("threshold_ma", "")]).await;
    assert_eq!(resp.status_code(), 303);
    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        None
    );
    let (name, value) = dn(ADMIN_DN);
    let html = server
        .get("/ui/devices/factory-01")
        .add_header(name, value)
        .await
        .text();
    assert!(html.contains("(fleet default)"), "{html}");
    assert!(!html.contains(r#"value="12000""#), "{html}");
}

#[tokio::test]
async fn threshold_form_sets_and_clears_the_min_on_time_too() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "extruder-01", "active");

    let mut req = server.post("/ui/devices/extruder-01/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req
        .form(&[
            ("csrf", "tok"),
            ("threshold_ma", "15000"),
            ("min_on_s", " 120 "),
        ])
        .await;
    assert_eq!(resp.status_code(), 303, "{}", resp.text());
    let stored = store.get_device("extruder-01").unwrap().unwrap();
    assert_eq!(
        (stored.power_threshold_ma, stored.min_on_s),
        (Some(15_000), Some(120))
    );

    let (name, value) = dn(ADMIN_DN);
    let html = server
        .get("/ui/devices/extruder-01")
        .add_header(name, value)
        .await
        .text();
    assert!(html.contains(r#"name="min_on_s""#), "{html}");
    assert!(html.contains(r#"value="120""#), "{html}");
    assert!(html.contains("producing after 120 s"), "{html}");
    let (name, value) = dn(ADMIN_DN);
    let fleet_html = server.get("/ui").add_header(name, value).await.text();
    assert!(fleet_html.contains("for 120 s"), "{fleet_html}");
    assert!(fleet_html.contains(r#"data-min-on="120""#), "{fleet_html}");

    // Bad values are 422 and write nothing; the cap holds.
    for bad in ["abc", "-1", "1.5", "3601", "٣"] {
        let mut req = server.post("/ui/devices/extruder-01/threshold");
        for (name, value) in admin_with_cookie("tok") {
            req = req.add_header(name, value);
        }
        let resp = req
            .form(&[
                ("csrf", "tok"),
                ("threshold_ma", "15000"),
                ("min_on_s", bad),
            ])
            .await;
        assert_eq!(resp.status_code(), 422, "{bad}: {}", resp.text());
        assert_eq!(
            store.get_device("extruder-01").unwrap().unwrap().min_on_s,
            Some(120),
            "{bad}"
        );
    }

    // Empty clears the min-on and keeps the threshold the form carries.
    let mut req = server.post("/ui/devices/extruder-01/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req
        .form(&[("csrf", "tok"), ("threshold_ma", "15000"), ("min_on_s", "")])
        .await;
    assert_eq!(resp.status_code(), 303);
    let stored = store.get_device("extruder-01").unwrap().unwrap();
    assert_eq!(
        (stored.power_threshold_ma, stored.min_on_s),
        (Some(15_000), None)
    );
    let (name, value) = dn(ADMIN_DN);
    let fleet_html = server.get("/ui").add_header(name, value).await.text();
    assert!(fleet_html.contains(r#"data-min-on="0""#), "{fleet_html}");
    assert!(!fleet_html.contains("for 120 s"), "{fleet_html}");
}

#[tokio::test]
async fn threshold_form_rejects_non_numbers_and_needs_csrf_and_admin() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    store
        .set_device_threshold("factory-01", Some(5_000), None)
        .unwrap();

    for bad in ["abc", "1.5", "12 000", "1e3", "٣"] {
        let mut req = server.post("/ui/devices/factory-01/threshold");
        for (name, value) in admin_with_cookie("tok") {
            req = req.add_header(name, value);
        }
        let resp = req.form(&[("csrf", "tok"), ("threshold_ma", bad)]).await;
        assert_eq!(resp.status_code(), 422, "{bad:?}: {}", resp.text());
    }

    // Wrong CSRF token.
    let mut req = server.post("/ui/devices/factory-01/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "other"), ("threshold_ma", "1")]).await;
    assert_eq!(resp.status_code(), 403);

    // Device certificate.
    let (name, value) = dn(DEVICE_DN);
    let resp = server
        .post("/ui/devices/factory-01/threshold")
        .add_header(name, value)
        .form(&[("csrf", "tok"), ("threshold_ma", "1")])
        .await;
    assert_eq!(resp.status_code(), 403);

    // Unknown device.
    let mut req = server.post("/ui/devices/ghost/threshold");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("threshold_ma", "1")]).await;
    assert_eq!(resp.status_code(), 404);

    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .power_threshold_ma,
        Some(5_000),
        "nothing above may have written"
    );
}

// ── Live windows ──────────────────────────────────────────────────────

/// Short windows (≤ 5 min) mark the page live — the script re-renders on
/// every uplink event and the timed reload is off; longer windows keep the
/// 60 s reload and are not live. Both history and device pages.
#[tokio::test]
async fn short_windows_are_live_long_windows_reload() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");

    for (path, live) in [
        ("/ui/history?window=1m", true),
        ("/ui/history?window=5m", true),
        ("/ui/history?window=15m", false),
        ("/ui/history", false),
        ("/ui/devices/factory-01?window=5m", true),
        ("/ui/devices/factory-01?window=1h", false),
    ] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server.get(path).add_header(name, value).await;
        assert_eq!(resp.status_code(), 200, "{path}");
        let html = resp.text();
        assert_eq!(html.contains(r#"data-live="1""#), live, "{path}: live flag");
        assert_eq!(
            html.contains("data-refresh="),
            !live,
            "{path}: timed reload"
        );
        // The filter row offers every preset, live ones included.
        for w in ["1m", "5m", "15m", "1h", "6h", "24h", "7d"] {
            assert!(html.contains(&format!("?window={w}\"")), "{path} lacks {w}");
        }
    }

    // The fleet table is neither: rows are patched in place by the stream.
    let (name, value) = dn(ADMIN_DN);
    let html = server.get("/ui").add_header(name, value).await.text();
    assert!(!html.contains(r#"data-live="1""#));
    assert!(!html.contains("data-refresh="));
    // The scripts that make it work ship on every page.
    assert!(
        html.contains("svBindCharts"),
        "chart binding is re-runnable"
    );
    assert!(html.contains("data-live"), "live script reads the flag");
    assert!(
        html.contains("DOMParser"),
        "live script swaps <main> from a fresh render"
    );
}

/// Fleet rows carry the threshold the live script judges `current` against,
/// so a pushed reading flips the badge without a round trip.
#[tokio::test]
async fn fleet_rows_carry_their_power_threshold() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "bench-01", "active");
    seed_device(store.as_ref(), "press-01", "active");
    store
        .set_device_threshold("press-01", Some(12_000), None)
        .unwrap();

    let (name, value) = dn(ADMIN_DN);
    let html = server.get("/ui").add_header(name, value).await.text();
    assert!(
        html.contains(r#"data-device="bench-01" data-threshold="1000""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-device="press-01" data-threshold="12000""#),
        "{html}"
    );
    assert!(html.contains("current-now"), "a slot for the live current");
}

#[tokio::test]
async fn status_rejects_values_outside_whitelist() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    for bad in ["revoked", "ACTIVE; DROP TABLE devices", ""] {
        let mut req = server.post("/ui/devices/factory-01/status");
        for (name, value) in admin_with_cookie("tok") {
            req = req.add_header(name, value);
        }
        let resp = req.form(&[("csrf", "tok"), ("status", bad)]).await;
        assert_eq!(resp.status_code(), 422, "status {bad:?} must be rejected");
    }
    assert_eq!(
        store.get_device("factory-01").unwrap().unwrap().status,
        "active"
    );
}

#[tokio::test]
async fn status_on_unknown_device_is_404() {
    let (server, _) = server_with_store();
    let mut req = server.post("/ui/devices/ghost/status");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("status", "inactive")]).await;
    assert_eq!(resp.status_code(), 404);
}

#[tokio::test]
async fn malformed_form_body_is_client_error_not_5xx() {
    let (server, _) = server_with_store();
    let mut req = server.post("/ui/devices");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req
        .content_type("application/x-www-form-urlencoded")
        .text("%%%not=a&valid%form\u{0}")
        .await;
    assert!(
        resp.status_code().is_client_error(),
        "malformed body must be 4xx, got {}",
        resp.status_code()
    );
}

// ── History page ──────────────────────────────────────────────────────

#[tokio::test]
async fn history_page_renders_tiles_status_chart_and_a_chart_per_metric() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    seed_device(store.as_ref(), "factory-02", "active");
    seed_device(store.as_ref(), "retired", "inactive");
    store
        .put_uplink(UplinkRecord {
            device_id: "factory-01".into(),
            received_at: supervictor_endpoint::time::now_rfc3339(),
            payload: serde_json::json!({
                "current": 1240, "temp_c": 23.5, "humidity_pct": 41.0,
                "rssi_dbm": -63, "heap_free": 90112, "fw": "0.1.0"
            }),
        })
        .unwrap();

    // Nothing selected: the fleet aggregates, the device chips, and a hint
    // where the traces would be. No device is plotted until picked.
    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui/history").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    // Filter row with every preset, the default marked.
    for w in ["15m", "1h", "6h", "24h", "7d"] {
        assert!(
            html.contains(&format!("href=\"/ui/history?window={w}\"")),
            "missing window link {w}"
        );
    }
    assert!(html.contains("class=\"active\" href=\"/ui/history?window=1h\""));
    // Headline tiles reflect the fleet right now.
    assert!(html.contains("active devices"));
    assert!(html.contains("3 registered, 1 inactive"));
    assert!(html.contains("<nav class=\"devices\""), "device chips");
    for id in ["factory-01", "factory-02", "retired"] {
        assert!(
            html.contains(&format!("data-device=\"{id}\"")),
            "chip for every registered device: {id}"
        );
    }
    assert!(html.contains("no devices plotted"), "{html}");
    assert_eq!(
        html.matches("<figure class=\"chart status-chart\"").count(),
        2,
        "power and status stacks are fleet-wide"
    );
    assert_eq!(
        html.matches("<figure class=\"chart\"").count(),
        0,
        "no line charts without a selection"
    );
    assert!(html.contains("data-refresh=\"60\""));

    // Both reporting devices selected: current first (with the threshold
    // line), then power, then status, then the other metrics; every chart
    // has a table twin.
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/ui/history?device=factory-01&device=factory-02")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    let pos = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    assert!(pos("id=\"current\"") < pos("id=\"power\""));
    assert!(pos("id=\"power\"") < pos("id=\"status\""));
    assert!(pos("id=\"status\"") < pos("id=\"temp_c\""));
    assert_eq!(
        html.matches("<figure class=\"chart status-chart\"").count(),
        2
    );
    assert!(
        html.contains("class=\"threshold\""),
        "threshold line on the current chart"
    );
    assert!(html.contains("on ≥ 1000 mA"));
    assert!(html.contains("powered on") && html.contains("powered off"));
    for key in ["current", "temp_c", "humidity_pct", "rssi_dbm", "heap_free"] {
        assert!(
            html.contains(&format!("<figure class=\"chart\" id=\"{key}\"")),
            "no chart for {key}"
        );
    }
    assert_eq!(html.matches("class=\"table-view\"").count(), 7);
    // Both selected devices appear as series (legend + end labels) and
    // their chips are marked; the retired one is a chip only.
    assert!(html.contains("class=\"legend\""));
    assert!(html.contains("class=\"end-label\""));
    assert_eq!(html.matches("class=\"chip on\"").count(), 2, "{html}");
    assert!(html.contains("aria-pressed=\"true\" data-device=\"factory-01\""));
    assert!(html.contains("aria-pressed=\"false\" data-device=\"retired\""));
    // The live value made it into the bucket table of the current chart.
    assert!(html.contains("1240"), "current value should be tabulated");
    assert!(!html.contains("NaN"), "no NaN may leak into the markup");
}

/// Chips are toggle links: each one's href is the current selection with
/// that device added or removed, so the page works without script and a
/// URL reproduces a view. Unknown or hostile ids in the query are ignored.
#[tokio::test]
async fn device_chips_toggle_the_selection_in_the_url() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "a-dev", "active");
    seed_device(store.as_ref(), "b,dev", "active");
    let get = |path: String| {
        let (name, value) = dn(ADMIN_DN);
        server.get(&path).add_header(name, value)
    };

    let html = get("/ui/history?window=5m".into()).await.text();
    assert!(
        html.contains("href=\"/ui/history?window=5m&amp;device=a-dev\""),
        "unselected chip adds itself: {html}"
    );
    assert!(
        html.contains("href=\"/ui/history?window=5m&amp;device=b%2Cdev\""),
        "ids are percent-encoded: {html}"
    );
    assert!(html.contains("data-live=\"1\""), "live window stays live");

    let html = get("/ui/history?window=5m&device=a-dev".into())
        .await
        .text();
    assert!(
        html.contains("href=\"/ui/history?window=5m&amp;device=\""),
        "deselecting the only device leaves an explicit empty selection: {html}"
    );
    assert!(
        html.contains("href=\"/ui/history?window=5m&amp;device=a-dev&amp;device=b%2Cdev\""),
        "the other chip adds itself to the current selection: {html}"
    );
    assert_eq!(html.matches("class=\"chip on\"").count(), 1);
    assert_eq!(html.matches("<figure class=\"chart\"").count(), 5);
    assert_eq!(
        deep(&html).matches("class=\"legend\"").count(),
        2,
        "one series: only the two stacks carry legends"
    );

    // Comma inside an id survives the round trip (repeated keys, not CSV).
    let html = get("/ui/history?device=b%2Cdev&device=a-dev".into())
        .await
        .text();
    assert_eq!(html.matches("class=\"chip on\"").count(), 2, "{html}");
    assert_eq!(
        deep(&html).matches("class=\"legend\"").count(),
        7,
        "two series: every line chart gets a legend too"
    );

    // Unknown and hostile ids: ignored, never reflected raw.
    let hostile = "<script>alert(1)</script>";
    let html = get(format!(
        "/ui/history?device={}&device=ghost&device=&device=a-dev",
        urlencoding(hostile)
    ))
    .await
    .text();
    assert!(!html.contains(hostile));
    assert_eq!(html.matches("class=\"chip on\"").count(), 1);
    assert!(html.contains("aria-pressed=\"true\" data-device=\"a-dev\""));
}

/// A stored style shows up wherever the device is drawn: the fleet table
/// key, the history traces and the device page; a custom colour rides on
/// `--c`, a palette slot on its class.
#[tokio::test]
async fn device_style_is_drawn_on_every_page() {
    use supervictor_common::models::{LineStyle, Marker};
    use supervictor_endpoint::models::DeviceStyle;
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "styled", "active");
    seed_device(store.as_ref(), "plain", "active");
    seed_device(store.as_ref(), "hexy", "active");
    store
        .set_device_style(
            "styled",
            DeviceStyle {
                color: Some("s3".into()),
                line_style: Some(LineStyle::Dashed),
                marker: Some(Marker::Circle),
            },
        )
        .unwrap();
    store
        .set_device_style(
            "hexy",
            DeviceStyle {
                color: Some("#ff8800".into()),
                line_style: None,
                marker: Some(Marker::Diamond),
            },
        )
        .unwrap();
    let now = supervictor_endpoint::time::now_rfc3339();
    for id in ["styled", "plain", "hexy"] {
        seed_uplink(store.as_ref(), id, &now, 500);
    }

    let (name, value) = dn(ADMIN_DN);
    let fleet = server.get("/ui").add_header(name, value).await.text();
    assert_eq!(
        deep(&fleet).matches("class=\"key-svg\"").count(),
        3,
        "a key per row"
    );
    assert!(fleet.contains("class=\"line s3 dashed\""), "{fleet}");
    assert!(
        fleet.contains("class=\"line custom solid\" style=\"--c:#ff8800\""),
        "{fleet}"
    );

    let (name, value) = dn(ADMIN_DN);
    let history = server
        .get("/ui/history?device=styled&device=plain&device=hexy")
        .add_header(name, value)
        .await
        .text();
    assert!(
        history.contains("<path class=\"line s3 dashed\""),
        "{history}"
    );
    assert!(
        history.contains("<circle class=\"mark s3\""),
        "circle markers at the points"
    );
    assert!(
        history.contains("<path class=\"mark custom\" style=\"--c:#ff8800\""),
        "diamond markers in the custom colour"
    );
    assert!(history.contains("<path class=\"line custom solid\" style=\"--c:#ff8800\""));
    assert!(
        history.contains("style=\"--c:var(--s3)\""),
        "chip wears the slot colour"
    );
    assert!(
        history.contains("style=\"--c:#ff8800\" role=\"button\""),
        "chip wears the hex"
    );
    // Legacy device (no stored colour) takes a palette slot positionally.
    assert!(
        history.contains("<path class=\"line s0 solid\""),
        "{history}"
    );
    // The hover data carries the style for the tooltip key.
    assert!(history.contains("&quot;dash&quot;:&quot;8 5&quot;"));
    assert!(history.contains("&quot;marker&quot;:&quot;diamond&quot;"));
    assert!(history.contains("&quot;color&quot;:&quot;#ff8800&quot;"));

    let (name, value) = dn(ADMIN_DN);
    let page = server
        .get("/ui/devices/styled")
        .add_header(name, value)
        .await
        .text();
    assert!(page.contains("<path class=\"line s3 dashed\""), "{page}");
    assert!(page.contains("chart style"), "the style form");
    assert!(
        page.contains("value=\"s3\" checked"),
        "stored colour preselected: {page}"
    );
    assert!(page.contains("value=\"dashed\" selected"));
    assert!(page.contains("value=\"circle\" selected"));
}

/// The style form writes through the same validation as the API: palette
/// slot or custom hex, a known line style and marker; defaults clear.
#[tokio::test]
async fn style_form_sets_clears_and_validates() {
    use supervictor_common::models::{LineStyle, Marker};
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    let post = |fields: &'static [(&'static str, &'static str)]| {
        let mut req = server.post("/ui/devices/factory-01/style");
        for (name, value) in admin_with_cookie("tok") {
            req = req.add_header(name, value);
        }
        req.form(fields)
    };

    let resp = post(&[
        ("csrf", "tok"),
        ("color", "s2"),
        ("line_style", "dotted"),
        ("marker", "square"),
    ])
    .await;
    assert_eq!(resp.status_code(), 303, "{}", resp.text());
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/ui/devices/factory-01"
    );
    let style = store.get_device("factory-01").unwrap().unwrap().style;
    assert_eq!(style.color.as_deref(), Some("s2"));
    assert_eq!(style.line_style, Some(LineStyle::Dotted));
    assert_eq!(style.marker, Some(Marker::Square));

    // Custom colour: the hex box counts only when "custom" is picked, and
    // is normalised.
    let resp = post(&[
        ("csrf", "tok"),
        ("color", "custom"),
        ("custom_color", "#ABCDEF"),
        ("line_style", "solid"),
        ("marker", "none"),
    ])
    .await;
    assert_eq!(resp.status_code(), 303, "{}", resp.text());
    let style = store.get_device("factory-01").unwrap().unwrap().style;
    assert_eq!(style.color.as_deref(), Some("#abcdef"));
    assert_eq!(
        style.line_style, None,
        "solid is the default, stored as unset"
    );
    assert_eq!(style.marker, None);
    let (name, value) = dn(ADMIN_DN);
    let page = server
        .get("/ui/devices/factory-01")
        .add_header(name, value)
        .await
        .text();
    assert!(page.contains("value=\"custom\" checked"), "{page}");
    assert!(
        page.contains("name=\"custom_color\" value=\"#abcdef\""),
        "{page}"
    );

    // "auto" clears the stored colour.
    let resp = post(&[("csrf", "tok"), ("color", ""), ("line_style", "dashdot")]).await;
    assert_eq!(resp.status_code(), 303);
    let style = store.get_device("factory-01").unwrap().unwrap().style;
    assert_eq!(style.color, None);
    assert_eq!(style.line_style, Some(LineStyle::DashDot));

    // Rejections leave the stored style alone.
    for bad in [
        &[
            ("csrf", "tok"),
            ("color", "custom"),
            ("custom_color", "red"),
        ][..],
        &[("csrf", "tok"), ("color", "s9")][..],
        &[("csrf", "tok"), ("line_style", "wavy")][..],
        &[("csrf", "tok"), ("marker", "star")][..],
    ] {
        let resp = post(bad).await;
        assert_eq!(resp.status_code(), 422, "{bad:?}: {}", resp.text());
    }
    assert_eq!(
        store
            .get_device("factory-01")
            .unwrap()
            .unwrap()
            .style
            .line_style,
        Some(LineStyle::DashDot)
    );
    // CSRF and admin gates.
    let resp = post(&[("csrf", "wrong"), ("color", "s1")]).await;
    assert_eq!(resp.status_code(), 403);
    let (name, value) = dn(DEVICE_DN);
    let resp = server
        .post("/ui/devices/factory-01/style")
        .add_header(name, value)
        .form(&[("csrf", "tok"), ("color", "s1")])
        .await;
    assert_eq!(resp.status_code(), 403);
    assert_eq!(
        store.get_device("factory-01").unwrap().unwrap().style.color,
        None
    );
    // Unknown device.
    let mut req = server.post("/ui/devices/ghost/style");
    for (name, value) in admin_with_cookie("tok") {
        req = req.add_header(name, value);
    }
    let resp = req.form(&[("csrf", "tok"), ("color", "s1")]).await;
    assert_eq!(resp.status_code(), 404);
}

#[tokio::test]
async fn history_page_rejects_bad_window_and_accepts_each_preset() {
    let (server, _) = server_with_store();
    for bad in ["2h", "1H", "1h%0A", "../etc/passwd", "%00"] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server
            .get(&format!("/ui/history?window={bad}"))
            .add_header(name, value)
            .await;
        assert_eq!(resp.status_code(), 422, "window={bad}");
    }
    for w in ["15m", "1h", "6h", "24h", "7d"] {
        let (name, value) = dn(ADMIN_DN);
        let resp = server
            .get(&format!("/ui/history?window={w}"))
            .add_header(name, value)
            .await;
        assert_eq!(resp.status_code(), 200, "window={w}");
        assert!(resp
            .text()
            .contains(&format!("class=\"active\" href=\"/ui/history?window={w}\"")));
    }
}

/// A device id is untrusted text: it reaches the legend, the end label,
/// the table header and the JSON in `data-chart`. It must be inert in all
/// four places — no `<script>` survives, no attribute breakout.
#[tokio::test]
async fn history_page_keeps_hostile_device_ids_inert_in_charts() {
    let (server, store) = server_with_store();
    let hostile = "x\"><script>alert(1)</script>";
    seed_device(store.as_ref(), hostile, "active");
    seed_uplink(
        store.as_ref(),
        hostile,
        &supervictor_endpoint::time::now_rfc3339(),
        7,
    );
    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui/history").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(!html.contains("<script>alert(1)"), "raw script tag leaked");
    assert!(
        !html.contains("x\"><script"),
        "attribute breakout: the hostile id's quote survived unescaped"
    );
    assert!(
        html.contains("&lt;script&gt;alert(1)"),
        "id should be escaped, not dropped"
    );
    // The device page for it is also fine.
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get(&format!("/ui/devices/{}", urlencoding(hostile)))
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 200);
    assert!(!resp.text().contains("<script>alert(1)"));
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::test]
async fn device_page_has_window_filters_and_metric_charts() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    seed_uplink(
        store.as_ref(),
        "factory-01",
        &supervictor_endpoint::time::now_rfc3339(),
        55,
    );
    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/ui/devices/factory-01?window=6h")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(html.contains("class=\"active\" href=\"/ui/devices/factory-01?window=6h\""));
    assert_eq!(html.matches("<figure class=\"chart\"").count(), 5);
    assert!(
        html.contains("badge power off"),
        "55 mA fresh reading is off: {html}"
    );
    assert!(html.contains("55 mA, threshold 1000 mA"));
    assert_eq!(
        deep(&html).matches("class=\"threshold\"").count(),
        1,
        "only the current chart"
    );
    assert!(
        !html.contains("class=\"legend\""),
        "a single series needs no legend"
    );
    assert!(html.contains("recent uplinks"));
    assert!(html.contains("&quot;current&quot;:55"));

    let (name, value) = dn(ADMIN_DN);
    let resp = server
        .get("/ui/devices/factory-01?window=bogus")
        .add_header(name, value)
        .await;
    assert_eq!(resp.status_code(), 422);
}

// ── Theme + phones ────────────────────────────────────────────────────

/// Every page carries the appearance button, the picker with one card per
/// theme (plus auto), and the pre-paint bootstrap; the stylesheet has a
/// token block per registered theme whose preview colours match, keeps the
/// `light` / `dark` aliases for choices stored by the old toggle, falls back
/// to the OS preference, and routes the stale colour through `--warn`.
#[tokio::test]
async fn every_page_has_the_theme_picker_and_every_theme_has_tokens() {
    use supervictor_endpoint::ui::themes::{Scheme, DEFAULT_DARK, DEFAULT_LIGHT, THEMES};
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    for path in ["/ui", "/ui/history", "/ui/live", "/ui/devices/factory-01"] {
        let (name, value) = dn(ADMIN_DN);
        let html = server.get(path).add_header(name, value).await.text();
        assert!(html.contains("id=\"theme-toggle\""), "{path}: no button");
        assert!(
            html.contains("aria-controls=\"theme-picker\"")
                && html.contains("<dialog id=\"theme-picker\""),
            "{path}: no picker"
        );
        assert!(html.contains("data-theme-id=\"auto\""), "{path}: auto card");
        for theme in THEMES {
            assert!(
                html.contains(&format!("data-theme-id=\"{}\"", theme.id)),
                "{path}: no card for {}",
                theme.id
            );
            assert!(
                html.contains(theme.name) && html.contains(theme.audience),
                "{path}: {}",
                theme.id
            );
        }
        let head_end = html.find("</head>").expect("head");
        let head = &html[..head_end];
        assert!(
            head.contains("sv-theme"),
            "{path}: theme bootstrap must run in <head>"
        );
        assert!(
            head.contains("sv-tweaks"),
            "{path}: tweak bootstrap must run in <head>"
        );
        assert!(
            head.contains("window.svThemes=[\"foundry\",\"clinical\""),
            "{path}: the id allow-list precedes the script"
        );
        assert!(
            head.contains("name=\"color-scheme\" content=\"dark light\""),
            "{path}"
        );
        // Tweak controls and the share box exist exactly once.
        for id in [
            "tw-accent",
            "tw-radius",
            "tw-density",
            "tw-font",
            "tw-texture",
            "tw-motion",
            "tw-json",
            "tw-import",
            "tw-copy-link",
            "tw-reset",
        ] {
            assert_eq!(
                html.matches(&format!("id=\"{id}\"")).count(),
                1,
                "{path}: {id}"
            );
        }
        assert!(
            !html.contains("<form method=\"dialog\""),
            "{path}: the picker is not a form"
        );
    }
    let (name, value) = dn(ADMIN_DN);
    let css = server
        .get("/ui/assets/style.css")
        .add_header(name, value)
        .await
        .text();
    let required = [
        "--bg",
        "--surface",
        "--border",
        "--text",
        "--muted",
        "--accent",
        "--warn",
        "--green",
        "--blue",
        "--red",
        "--font",
        "--font-display",
        "--radius",
        "--radius-lg",
        "--label-case",
        "--card-shadow",
        "--card-top",
        "--card-left",
        "--header-rule",
        "--glow",
        "--texture",
    ];
    for theme in THEMES {
        let selector = format!(":root[data-theme=\"{}\"]", theme.id);
        // The selector also appears in the shared series rule and the
        // signatures; the token block is the one declaring color-scheme.
        let block = css
            .match_indices(&selector)
            .map(|(at, _)| {
                let open = at + css[at..].find('{').unwrap();
                let close = open + css[open..].find('}').unwrap();
                &css[open..close]
            })
            .find(|block| block.contains("color-scheme:"))
            .unwrap_or_else(|| panic!("no token block for {}", theme.id));
        for token in required {
            assert!(
                block.contains(&format!("{token}:")),
                "{}: missing {token}",
                theme.id
            );
        }
        assert!(
            block.contains(&format!("color-scheme: {}", theme.scheme.css())),
            "{}",
            theme.id
        );
        for (token, hex) in [
            ("--bg", theme.preview.bg),
            ("--surface", theme.preview.surface),
            ("--accent", theme.preview.accent),
            ("--text", theme.preview.text),
        ] {
            assert!(
                block.contains(&format!("{token}: {hex}")),
                "{}: preview {token} {hex} is not the CSS token",
                theme.id
            );
        }
        // Series slots are the validated reference steps for the surface.
        let series_block = if theme.scheme == Scheme::Dark {
            "--s0: #3987e5"
        } else {
            "--s0: #2a78d6"
        };
        let series_at = css.find(series_block).unwrap();
        let series_rule = &css[..series_at];
        let selectors = &series_rule[series_rule.rfind('}').map_or(0, |i| i + 1)..];
        assert!(
            selectors.contains(&selector),
            "{}: not in the {:?} series rule",
            theme.id,
            theme.scheme
        );
    }
    // Aliases + OS fallback.
    assert!(
        css.contains(":root[data-theme=\"dark\"]") && css.contains(":root[data-theme=\"light\"]")
    );
    assert!(css.contains(&format!(
        ":root, :root[data-theme=\"{DEFAULT_DARK}\"], :root[data-theme=\"dark\"]"
    )));
    assert!(css.contains(&format!(
        ":root[data-theme=\"{DEFAULT_LIGHT}\"], :root[data-theme=\"light\"]"
    )));
    assert!(css.contains("@media (prefers-color-scheme: light)"));
    assert!(css.contains(":root:not([data-theme])"));
    // Semantics: stale is --warn, never the brand accent; tweaks compose.
    assert!(css.contains(".badge.stale { color: var(--warn); }"));
    assert!(css.contains(".tile.stale .tile-value { color: var(--warn); }"));
    assert!(
        !css.contains("stale { color: var(--accent)"),
        "stale must not borrow the brand colour"
    );
    assert!(css.contains(":root[data-texture=\"off\"] body { background-image: none; }"));
    assert!(css.contains(":root[data-motion=\"off\"]"));
    assert!(css.contains("html { font-size: calc(16px * var(--density)); }"));
    assert!(
        !css.contains("border-radius: 4px") && !css.contains("border-radius: 6px"),
        "radii are tokens"
    );
    assert!(css.contains("@media (max-width: 640px)"));
    // Two layers: the glance is phone-only, the deep layer hides on phones
    // until the viewer lifts it.
    assert!(css.contains(".glance, #view-toggle { display: none; }"));
    assert!(css.contains(".deep { display: none; }"));
    assert!(css.contains(":root[data-view=\"full\"] .deep { display: block; }"));
    assert!(css.contains(":root[data-view=\"full\"] .glance { display: none; }"));
    assert!(css.contains("figure.chart.narrow svg { min-width: 0; }"));
    // Live re-renders no longer wash the whole page green; chips, dash
    // patterns and custom colours have their rules.
    assert!(!css.contains("main.tick"), "no page-wide flash");
    assert!(
        css.contains("tr.tick td"),
        "row flash on the fleet table stays"
    );
    assert!(css.contains(".devices a.chip.on"));
    assert!(
        css.contains(".line.dashed")
            && css.contains(".line.dotted")
            && css.contains(".line.dashdot")
    );
    assert!(css.contains(".custom { stroke: var(--c)"));
}

// ── Live view ─────────────────────────────────────────────────────────

fn seed_burst(store: &dyn DeviceStore, id: &str, at: &str, levels: Vec<u8>, ms: u32) {
    store
        .put_uplink(UplinkRecord {
            device_id: id.into(),
            received_at: at.into(),
            payload: serde_json::json!({ "current": 1500, "levels": levels, "levels_ms": ms }),
        })
        .unwrap();
}

#[tokio::test]
async fn live_page_has_a_meter_per_device_per_register_and_a_strip_per_register() {
    use supervictor_common::levels::{COUNT, REGISTERS};
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "bursty", "active");
    seed_device(store.as_ref(), "quiet", "active");
    seed_device(store.as_ref(), "plain", "inactive");
    let now = supervictor_endpoint::time::now_rfc3339();
    // bursty: three samples, the newest with current at level 200.
    let mut flat = vec![1u8; COUNT * 3];
    flat[COUNT * 2] = 200;
    seed_burst(store.as_ref(), "bursty", &now, flat, 500);
    seed_uplink(store.as_ref(), "plain", &now, 3000);

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui/live").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(html.contains("id=\"live\""), "live root missing");
    assert!(html.contains("data-poll=\"/ui/live.json\""));
    assert!(html.contains("href=\"/ui/live\""), "nav link");
    assert_eq!(
        html.matches("class=\"meter\"").count(),
        3 * COUNT,
        "one meter per device per register"
    );
    assert_eq!(
        html.matches("class=\"strip wide\"").count() + html.matches("class=\"strip deep\"").count(),
        COUNT,
        "one strip per register; only the first is in the phone glance"
    );
    for r in &REGISTERS {
        assert!(html.contains(r.label), "{} strip missing", r.label);
    }
    // bursty's newest current level is 200: a meter at --lv:200 with the
    // dequantised value; quiet has no sample: level 0 renders as "–".
    assert!(
        html.contains("--lv:200"),
        "newest level should drive the meter"
    );
    let expected = supervictor_endpoint::ui::charts::fmt_value(f64::from(
        REGISTERS[0].dequantize(200).unwrap(),
    ));
    assert!(
        html.contains(&format!("<span class=\"meter-value\">{expected}</span>")),
        "dequantised readout"
    );
    assert!(
        html.contains("--lv:0"),
        "a device with no sample reads empty"
    );
    // plain's scalar current 3000 is quantised for its meter.
    let lv = REGISTERS[0].quantize(3000.0);
    assert!(
        html.contains(&format!("--lv:{lv}")),
        "scalar fallback quantised"
    );
    // The seed carries the register table and the samples.
    assert!(html.contains("&quot;registers&quot;"));
    assert!(html.contains("&quot;window_s&quot;:120"));
    // Legend for 2+ devices, span buttons, and no inline user data in scripts.
    assert!(html.contains("class=\"legend\""));
    assert!(html.contains("data-span=\"60\""));
    // The raw 8-bit level is never a readout: no level span, and the ARIA
    // range is the register's real units, not 1..=255.
    assert!(
        !html.contains("meter-level"),
        "raw level span must not render"
    );
    assert!(
        !html.contains("aria-valuemax=\"255\""),
        "ARIA range must be in real units"
    );
    let v = f64::from(REGISTERS[0].dequantize(200).unwrap());
    assert!(
        html.contains(&format!("aria-valuenow=\"{v}\"")),
        "aria-valuenow should be the dequantised value, got no {v} in page"
    );
    assert!(
        html.contains(&format!(
            "aria-valuetext=\"{expected} {}\"",
            REGISTERS[0].unit
        )),
        "aria-valuetext is the readout plus unit"
    );
    assert!(
        html.contains(&format!("aria-valuemax=\"{}\"", REGISTERS[0].hi)),
        "aria-valuemax is the register's full scale"
    );
}

#[tokio::test]
async fn live_page_keeps_hostile_device_ids_inert_in_seed_and_bank() {
    let (server, store) = server_with_store();
    let hostile = "x\"><script>alert(1)</script>";
    seed_device(store.as_ref(), hostile, "active");
    seed_uplink(
        store.as_ref(),
        hostile,
        &supervictor_endpoint::time::now_rfc3339(),
        7,
    );
    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui/live").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let html = resp.text();
    assert!(!html.contains("<script>alert(1)"), "raw script tag leaked");
    assert!(!html.contains("x\"><script"), "attribute breakout");
    assert!(
        html.contains("&lt;script&gt;alert(1)"),
        "id escaped, not dropped"
    );
}

#[tokio::test]
async fn live_json_is_the_page_seed_with_expanded_bursts() {
    use supervictor_common::levels::COUNT;
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "b", "active");
    seed_device(store.as_ref(), "a", "active");
    let now_ms = supervictor_endpoint::time::now_unix_ms();
    let at = supervictor_endpoint::time::format_rfc3339(
        now_ms / 1000,
        (now_ms % 1000) as u32 * 1_000_000,
    );
    let flat: Vec<u8> = (1..=(COUNT * 4) as u8).collect();
    seed_burst(store.as_ref(), "b", &at, flat, 250);
    // A row two minutes and more ago is outside the window.
    let old = supervictor_endpoint::time::format_rfc3339(now_ms / 1000 - 130, 0);
    seed_uplink(store.as_ref(), "a", &old, 1);

    let (name, value) = dn(ADMIN_DN);
    let resp = server.get("/ui/live.json").add_header(name, value).await;
    assert_eq!(resp.status_code(), 200);
    let json: serde_json::Value = resp.json();
    assert_eq!(json["window_s"], 120);
    assert!(json["now_ms"].as_u64().unwrap() >= now_ms);
    let registers = json["registers"].as_array().unwrap();
    assert_eq!(registers.len(), COUNT);
    assert_eq!(registers[0]["key"], "current");
    assert!(registers[0]["hi"].as_f64().unwrap() > registers[0]["lo"].as_f64().unwrap());
    let devices = json["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["device_id"], "a", "sorted by id");
    assert_eq!(devices[0]["style"]["class"], "s0");
    assert_eq!(devices[0]["style"]["color"], "var(--s0)");
    assert!(devices[0]["style"]["dash"].is_null());
    assert_eq!(devices[0]["style"]["marker"], "none");
    assert_eq!(
        devices[0]["samples"].as_array().unwrap().len(),
        0,
        "old row excluded"
    );
    let samples = devices[1]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 4, "burst expanded");
    let t: Vec<u64> = samples.iter().map(|s| s["t"].as_u64().unwrap()).collect();
    assert!(t.windows(2).all(|w| w[1] == w[0] + 250), "{t:?}");
    assert_eq!(samples[0]["l"], serde_json::json!([1, 2, 3, 4, 5]));
    assert_eq!(samples[3]["l"][COUNT - 1], (COUNT * 4) as u64);
}

/// The live page draws each device in its stored style: the bank key and
/// meter bar, the legend, and the seed the canvas script reads.
#[tokio::test]
async fn live_page_uses_the_stored_device_style() {
    use supervictor_common::models::{LineStyle, Marker};
    use supervictor_endpoint::models::DeviceStyle;
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "hexy", "active");
    seed_device(store.as_ref(), "plain", "active");
    store
        .set_device_style(
            "hexy",
            DeviceStyle {
                color: Some("#ff8800".into()),
                line_style: Some(LineStyle::Dashed),
                marker: Some(Marker::Triangle),
            },
        )
        .unwrap();

    let (name, value) = dn(ADMIN_DN);
    let html = server.get("/ui/live").add_header(name, value).await.text();
    assert!(
        html.contains("class=\"meter-bar custom\" style=\"--c:#ff8800\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"meter-bar s0\""),
        "legacy device on a slot"
    );
    assert!(
        html.contains("class=\"line custom dashed\" style=\"--c:#ff8800\""),
        "bank key"
    );
    assert!(
        html.contains("class=\"mark custom\" style=\"--c:#ff8800\""),
        "triangle in the key"
    );
    assert!(
        html.contains("&quot;dash&quot;:&quot;8 5&quot;"),
        "seed carries the dash"
    );
    assert!(html.contains("&quot;marker&quot;:&quot;triangle&quot;"));

    let (name, value) = dn(ADMIN_DN);
    let json: serde_json::Value = server
        .get("/ui/live.json")
        .add_header(name, value)
        .await
        .json();
    let hexy = json["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_id"] == "hexy")
        .unwrap();
    assert_eq!(hexy["style"]["class"], "custom");
    assert_eq!(hexy["style"]["color"], "#ff8800");
    assert_eq!(hexy["style"]["dash"], "8 5");
    assert_eq!(hexy["style"]["marker"], "triangle");
}

// ── Phone glance ──────────────────────────────────────────────────────

/// Every page carries a `.glance` layer ahead of its `.deep` layer: the
/// fleet's verdict and one tappable row per device, a device's state and
/// its current trace drawn at phone width, the fleet-wide stacks narrow,
/// and a live card per device whose meters the live script also updates.
/// The header marks the current tab and the view toggle is on every page.
#[tokio::test]
async fn every_page_has_a_phone_glance_ahead_of_the_deep_layer() {
    use supervictor_common::levels::COUNT;
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "pump", "active");
    seed_device(store.as_ref(), "idle", "active");
    seed_device(store.as_ref(), "retired", "inactive");
    seed_device(store.as_ref(), "<b>x</b>", "active");
    let now = supervictor_endpoint::time::now_rfc3339();
    seed_uplink(store.as_ref(), "pump", &now, 1500);
    seed_uplink(store.as_ref(), "idle", &now, 0);
    seed_uplink(store.as_ref(), "retired", "2026-01-01T00:00:00+00:00", 900);
    let get = |path: &'static str| {
        let (name, value) = dn(ADMIN_DN);
        server.get(path).add_header(name, value)
    };

    // Fleet: verdict first (one off beats two... the off machine is the
    // headline), then a row per device with its threshold for the live
    // script, relative times coloured by freshness, inactive dimmed.
    let html = get("/ui").await.text();
    let glance_end = html.find("<div class=\"deep\">").expect("deep layer");
    let glance = &html[..glance_end];
    assert!(glance.contains("<section class=\"glance\">"));
    assert!(
        glance.contains("<section class=\"verdict off\""),
        "{glance}"
    );
    assert!(glance.contains("<span class=\"verdict-head\">1 off</span>"));
    assert!(glance.contains("1 on · 1 off · 1 never heard · 1 inactive"));
    assert_eq!(glance.matches("class=\"glance-row\"").count(), 4);
    assert_eq!(glance.matches("data-threshold=\"1000\"").count(), 4);
    assert!(glance.contains("<li class=\"inactive\" data-device=\"retired\""));
    assert!(glance.contains("<span class=\"badge inactive\">inactive</span>"));
    assert_eq!(glance.matches("class=\"when fresh\">just now<").count(), 2);
    assert!(glance.contains("class=\"when unknown\">never<"));
    assert!(
        glance.contains("class=\"when dark\">") && glance.contains(" d ago<"),
        "{glance}"
    );
    assert!(glance.contains("&lt;b&gt;x&lt;/b&gt;") && !glance.contains("<b>x</b>"));
    assert!(html.contains("id=\"view-toggle\""));
    assert!(html.contains("class=\"current\" href=\"/ui\" aria-current=\"page\""));
    assert!(!html.contains("class=\"current\" href=\"/ui/live\""));
    assert!(deep(&html).contains("register device"), "forms are desktop");

    // Device: the hero state, facts, window filters and a narrow current
    // chart with the threshold but no table twin; the forms stay deep.
    let html = get("/ui/devices/pump?window=6h").await.text();
    let glance_end = html.find("<div class=\"deep\">").expect("deep layer");
    let glance = &html[..glance_end];
    assert!(glance.contains("<div class=\"hero on\">"), "{glance}");
    assert!(glance.contains("<span class=\"hero-state\">on</span>"));
    assert!(glance.contains("<span class=\"hero-current\">1500 mA</span>"));
    assert!(glance.contains("on ≥ 1000 mA"));
    assert!(glance.contains("<dd class=\"when fresh\">just now</dd>"));
    assert!(glance.contains("1 in 6h"));
    assert!(glance.contains("class=\"active\" href=\"/ui/devices/pump?window=6h\""));
    assert!(glance.contains("<figure class=\"chart narrow\" id=\"current-glance\""));
    assert!(glance.contains("viewBox=\"0 0 340 120\""));
    assert!(glance.contains("class=\"threshold\""));
    assert!(
        !glance.contains("table-view"),
        "no table twin in the glance"
    );
    assert!(
        !glance.contains("end-label"),
        "no direct labels in the glance"
    );
    assert!(!glance.contains("<form"), "no forms in the glance");
    assert!(deep(&html).contains("<form"));

    // History: verdict and the two stacks narrow; chips and tiles deep.
    let html = get("/ui/history").await.text();
    let glance_end = html.find("<div class=\"deep\">").expect("deep layer");
    let glance = &html[..glance_end];
    assert!(glance.contains("<section class=\"verdict off\""));
    assert!(glance.contains("<figure class=\"chart status-chart narrow\" id=\"power-glance\""));
    assert!(glance.contains("<figure class=\"chart status-chart narrow\" id=\"status-glance\""));
    assert!(!glance.contains("table-view"));
    assert!(!glance.contains("class=\"chip"), "chips are desktop");
    assert!(deep(&html).contains("<nav class=\"devices\""));
    assert!(html.contains("class=\"current\" href=\"/ui/history\" aria-current=\"page\""));

    // Live: a card per device — a hero meter for the first register, a
    // mini for each other one — and only the first strip in the glance.
    let html = get("/ui/live").await.text();
    let glance_end = html.find("<div class=\"deep\">").expect("deep layer");
    let glance = &html[..glance_end];
    assert_eq!(
        glance
            .matches("<li class=\"live-card\" data-device=")
            .count(),
        4
    );
    assert_eq!(glance.matches("class=\"meter hero-meter\"").count(), 4);
    assert_eq!(
        glance.matches("class=\"meter mini\"").count(),
        4 * (COUNT - 1)
    );
    assert_eq!(
        html.matches("class=\"meter\"").count(),
        4 * COUNT,
        "the bank's meters are unchanged"
    );
    assert!(
        glance.contains("--lv:0"),
        "a device with no sample reads empty"
    );
    assert!(glance.contains("&lt;b&gt;x&lt;/b&gt;") && !glance.contains("<b>x</b>"));
    assert_eq!(html.matches("class=\"strip wide\"").count(), 1);
    assert_eq!(html.matches("class=\"strip deep\"").count(), COUNT - 1);
    assert!(html.contains("class=\"current\" href=\"/ui/live\" aria-current=\"page\""));
}

/// The history tiles describe what "on" was judged against: the fleet
/// default while nobody overrides it, "per-device thresholds" as soon as
/// one device carries its own (the default alone would misdescribe it).
#[tokio::test]
async fn history_tiles_name_per_device_thresholds_once_one_is_set() {
    use supervictor_endpoint::fleet::DEFAULT_POWER_THRESHOLD_MA;
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "bench-01", "active");
    seed_device(store.as_ref(), "press-01", "active");
    seed_device(store.as_ref(), "retired", "inactive");
    let now = supervictor_endpoint::time::now_rfc3339();
    seed_uplink(store.as_ref(), "bench-01", &now, 9_000);
    seed_uplink(store.as_ref(), "press-01", &now, 9_000);

    let get = || async {
        let (name, value) = dn(ADMIN_DN);
        server
            .get("/ui/history")
            .add_header(name, value)
            .await
            .text()
    };
    // Only the tiles are under test: the power chart keeps its own
    // "on = current ≥ N mA" note (fleet default by design).
    let tiles = |html: &str| -> String {
        let at = html.find("<section class=\"tiles\">").expect("tiles");
        let end = html[at..].find("</section>").expect("tiles end");
        html[at..at + end].to_string()
    };
    let html = tiles(&get().await);
    let default_note = format!("current ≥ {DEFAULT_POWER_THRESHOLD_MA} mA");
    assert!(html.contains(&default_note), "{html}");
    assert!(!html.contains("per-device thresholds"), "{html}");

    // One override (press-01 idles above the default): the tiles say so,
    // counting only active devices.
    store
        .set_device_threshold("press-01", Some(12_000), None)
        .unwrap();
    store
        .set_device_threshold("retired", Some(5), None)
        .unwrap();
    let html = tiles(&get().await);
    let note = format!(
        "per-device thresholds — 1 of 2 set, fleet default {DEFAULT_POWER_THRESHOLD_MA} mA"
    );
    assert!(html.contains(&note), "{html}");
    assert!(html.contains("below their thresholds"), "{html}");
    assert!(!html.contains(&default_note), "{html}");

    // The API summary carries the same count.
    let (name, value) = dn(ADMIN_DN);
    let summary: serde_json::Value = server
        .get("/fleet/summary")
        .add_header(name, value)
        .await
        .json();
    assert_eq!(summary["threshold_overrides"], 1, "{summary}");
    assert_eq!(summary["on"], 1, "bench-01 at 9 A: {summary}");
    assert_eq!(summary["off"], 1, "press-01 under 12 A: {summary}");
}
