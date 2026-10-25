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
        })
        .unwrap();
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
    // One status chart plus one chart per metric, each with a table twin.
    assert_eq!(
        html.matches("<figure class=\"chart status-chart\"").count(),
        1
    );
    for key in ["current", "temp_c", "humidity_pct", "rssi_dbm", "heap_free"] {
        assert!(
            html.contains(&format!("<figure class=\"chart\" id=\"{key}\"")),
            "no chart for {key}"
        );
    }
    assert_eq!(html.matches("class=\"table-view\"").count(), 6);
    // Both active devices appear as series (legend + end labels); the
    // retired one is in the series list too (every registered device).
    assert!(html.contains("factory-01"));
    assert!(html.contains("factory-02"));
    // The live value made it into the bucket table of the current chart.
    assert!(html.contains("1240"), "current value should be tabulated");
    assert!(html.contains("data-refresh=\"60\""));
    assert!(!html.contains("NaN"), "no NaN may leak into the markup");
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

/// Every page carries the theme toggle and the pre-paint bootstrap; the
/// stylesheet defines light tokens for both the explicit toggle and the OS
/// preference, plus a phone breakpoint.
#[tokio::test]
async fn every_page_has_the_theme_toggle_and_light_tokens_exist() {
    let (server, store) = server_with_store();
    seed_device(store.as_ref(), "factory-01", "active");
    for path in ["/ui", "/ui/history", "/ui/devices/factory-01"] {
        let (name, value) = dn(ADMIN_DN);
        let html = server.get(path).add_header(name, value).await.text();
        assert!(html.contains("id=\"theme-toggle\""), "{path}: no toggle");
        assert!(
            html.contains("class=\"icon-sun\"") && html.contains("class=\"icon-moon\""),
            "{path}: icons"
        );
        let head_end = html.find("</head>").expect("head");
        assert!(
            html[..head_end].contains("sv-theme"),
            "{path}: theme bootstrap must run in <head>"
        );
        assert!(
            html.contains("name=\"color-scheme\" content=\"dark light\""),
            "{path}"
        );
    }
    let (name, value) = dn(ADMIN_DN);
    let css = server
        .get("/ui/assets/style.css")
        .add_header(name, value)
        .await
        .text();
    assert!(css.contains(":root[data-theme=\"light\"]"));
    assert!(css.contains("@media (prefers-color-scheme: light)"));
    assert!(css.contains(":root:not([data-theme=\"dark\"])"));
    assert!(css.contains("@media (max-width: 640px)"));
}
