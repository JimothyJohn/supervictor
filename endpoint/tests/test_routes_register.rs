mod common;

use axum::http::StatusCode;
use axum_test::TestServer;
use supervictor_endpoint::routes;

fn test_server() -> TestServer {
    let store = common::test_store();
    let app = routes::router(store);
    TestServer::new(app)
}

#[tokio::test]
async fn register_device() {
    let server = test_server();
    let resp = server
        .post("/devices")
        .json(&serde_json::json!({
            "device_id": "dev-1",
            "owner_id": "owner-1"
        }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["device_id"], "dev-1");
    assert_eq!(body["status"], "active");
}

#[tokio::test]
async fn register_device_duplicate() {
    let server = test_server();
    let payload = serde_json::json!({
        "device_id": "dev-1",
        "owner_id": "owner-1"
    });
    server.post("/devices").json(&payload).await;
    let resp = server.post("/devices").json(&payload).await;
    resp.assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn register_device_empty_body() {
    let server = test_server();
    let resp = server.post("/devices").await;
    resp.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_device_invalid_json() {
    let server = test_server();
    let resp = server.post("/devices").text("{bad}").await;
    resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_devices_empty() {
    let server = test_server();
    let resp = server.get("/devices").await;
    resp.assert_status_ok();
    let body: Vec<serde_json::Value> = resp.json();
    assert!(body.is_empty());
}

#[tokio::test]
async fn list_devices_with_data() {
    let server = test_server();
    server
        .post("/devices")
        .json(&serde_json::json!({"device_id": "dev-1", "owner_id": "o1"}))
        .await;
    server
        .post("/devices")
        .json(&serde_json::json!({"device_id": "dev-2", "owner_id": "o2"}))
        .await;

    let resp = server.get("/devices").await;
    resp.assert_status_ok();
    let body: Vec<serde_json::Value> = resp.json();
    assert_eq!(body.len(), 2);
}

#[tokio::test]
async fn get_device_found() {
    let server = test_server();
    server
        .post("/devices")
        .json(&serde_json::json!({"device_id": "dev-1", "owner_id": "o1"}))
        .await;

    let resp = server.get("/devices/dev-1").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["device_id"], "dev-1");
}

#[tokio::test]
async fn get_device_not_found() {
    let server = test_server();
    let resp = server.get("/devices/nonexistent").await;
    resp.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_device_uplinks_empty() {
    let server = test_server();
    let resp = server.get("/devices/dev-1/uplinks").await;
    resp.assert_status_ok();
    let body: Vec<serde_json::Value> = resp.json();
    assert!(body.is_empty());
}

#[tokio::test]
async fn full_roundtrip() {
    let server = test_server();

    // Register
    server
        .post("/devices")
        .json(&serde_json::json!({"device_id": "dev-1", "owner_id": "o1"}))
        .await
        .assert_status(StatusCode::CREATED);

    // Uplink
    server
        .post("/")
        .json(&serde_json::json!({"id": "dev-1", "current": 99}))
        .await
        .assert_status_ok();

    // Fetch uplinks
    let resp = server.get("/devices/dev-1/uplinks").await;
    resp.assert_status_ok();
    let body: Vec<serde_json::Value> = resp.json();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["payload"]["current"], 99);
}
