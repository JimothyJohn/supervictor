//! The device API (`POST /devices`, `GET /devices`, `GET /devices/{id}`,
//! `GET /devices/{id}/uplinks`) is admin-only. Every board carries a
//! fleet-CA certificate in flash, so "any valid client certificate" is not
//! an authorization: one extracted key could register an untaken device id
//! bound to its own DN, list every device and owner, and read any device's
//! uplinks (decided 2026-11-15). A device certificate may uplink and read
//! its own firmware, nothing else.

mod common;

use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum_test::{TestResponse, TestServer};
use supervictor_endpoint::routes;

const SUBJECT_HEADER: &str = "x-ssl-client-subject-dn";
const ADMIN_DN: &str = "CN=ops,OU=admin,O=supervictor";
const DEVICE_DN: &str = "CN=dev-1,O=Supervictor,OU=devices";

/// Subjects that must not pass as an admin: a real device, and the
/// lookalikes a substring test would admit.
const NON_ADMIN_DNS: &[&str] = &[
    DEVICE_DN,
    "CN=OU=admin,O=Supervictor,OU=devices",
    "CN=evil,OU=admins",
    "CN=admin,O=admin",
    "CN=evil,OU=admin2",
    "",
];

fn server() -> TestServer {
    TestServer::new(routes::router(common::test_store()))
}

fn subject(dn: &str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(SUBJECT_HEADER),
        HeaderValue::from_str(dn).unwrap(),
    )
}

async fn get(server: &TestServer, path: &str, dn: Option<&str>) -> TestResponse {
    let req = server.get(path);
    match dn {
        Some(dn) => {
            let (name, value) = subject(dn);
            req.add_header(name, value).await
        }
        None => req.await,
    }
}

async fn register(server: &TestServer, device_id: &str, dn: Option<&str>) -> TestResponse {
    let req = server.post("/devices").json(&serde_json::json!({
        "device_id": device_id,
        "owner_id": "owner-1",
        "subject_dn": format!("CN={device_id},O=Supervictor,OU=devices"),
    }));
    match dn {
        Some(dn) => {
            let (name, value) = subject(dn);
            req.add_header(name, value).await
        }
        None => req.await,
    }
}

/// The device ids an admin sees, to prove a refused call changed nothing.
async fn registered_ids(server: &TestServer) -> Vec<String> {
    let resp = get(server, "/devices", Some(ADMIN_DN)).await;
    resp.assert_status_ok();
    let mut ids: Vec<String> = resp
        .json::<Vec<serde_json::Value>>()
        .iter()
        .map(|d| d["device_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn admin_registers_lists_and_reads() {
    let server = server();
    register(&server, "dev-1", Some(ADMIN_DN))
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(registered_ids(&server).await, ["dev-1"]);
    get(&server, "/devices/dev-1", Some(ADMIN_DN))
        .await
        .assert_status_ok();
    get(&server, "/devices/dev-1/uplinks", Some(ADMIN_DN))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn device_certificate_cannot_register_a_device() {
    let server = server();
    for dn in NON_ADMIN_DNS {
        // Its own id, and an id that is not taken yet: the takeover the
        // gate exists to stop.
        for id in ["dev-1", "untaken-id"] {
            register(&server, id, Some(dn))
                .await
                .assert_status(StatusCode::FORBIDDEN);
        }
    }
    assert!(
        registered_ids(&server).await.is_empty(),
        "a refused registration must not create a device"
    );
}

#[tokio::test]
async fn request_without_a_certificate_cannot_register_a_device() {
    let server = server();
    register(&server, "dev-1", None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    assert!(registered_ids(&server).await.is_empty());
}

#[tokio::test]
async fn device_certificate_cannot_list_devices() {
    let server = server();
    register(&server, "dev-1", Some(ADMIN_DN)).await;
    for dn in NON_ADMIN_DNS {
        let resp = get(&server, "/devices", Some(dn)).await;
        resp.assert_status(StatusCode::FORBIDDEN);
        assert!(
            !resp.text().contains("owner-1"),
            "a refusal must not carry the inventory"
        );
    }
    get(&server, "/devices", None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn device_certificate_cannot_read_a_device_record() {
    let server = server();
    register(&server, "dev-1", Some(ADMIN_DN)).await;
    register(&server, "dev-2", Some(ADMIN_DN)).await;
    // Neither another device's record nor its own, and an unknown id
    // answers the same 403 so the route is no existence oracle.
    for path in ["/devices/dev-2", "/devices/dev-1", "/devices/ghost"] {
        for dn in NON_ADMIN_DNS {
            get(&server, path, Some(dn))
                .await
                .assert_status(StatusCode::FORBIDDEN);
        }
        get(&server, path, None)
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn device_certificate_cannot_read_uplinks() {
    let server = server();
    register(&server, "dev-1", Some(ADMIN_DN)).await;
    register(&server, "dev-2", Some(ADMIN_DN)).await;
    for path in [
        "/devices/dev-2/uplinks",
        "/devices/dev-1/uplinks",
        "/devices/ghost/uplinks",
    ] {
        for dn in NON_ADMIN_DNS {
            get(&server, path, Some(dn))
                .await
                .assert_status(StatusCode::FORBIDDEN);
        }
        get(&server, path, None)
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
}

/// The gate must not take the device's own routes with it.
#[tokio::test]
async fn device_certificate_still_uplinks() {
    let server = server();
    register(&server, "dev-1", Some(ADMIN_DN)).await;
    let (name, value) = subject(DEVICE_DN);
    server
        .post("/")
        .add_header(name.clone(), value.clone())
        .json(&serde_json::json!({"id": "dev-1", "current": 100}))
        .await
        .assert_status_ok();
    server
        .get("/")
        .add_header(name, value)
        .await
        .assert_status_ok();
    server.get("/health").await.assert_status_ok();
}
