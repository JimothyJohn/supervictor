mod common;

use axum_test::TestServer;
use supervictor_endpoint::routes;

fn test_server() -> TestServer {
    let store = common::test_store();
    let app = routes::router(store);
    TestServer::new(app)
}

#[tokio::test]
async fn health_check() {
    let server = test_server();
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    resp.assert_json(&serde_json::json!({ "status": "ok" }));
}

#[tokio::test]
async fn hello_get() {
    let server = test_server();
    let resp = server.get("/").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["message"], "Hello from Supervictor!");
    assert!(body.get("client_subject").is_none());
}

#[tokio::test]
async fn hello_with_proxy_cert() {
    let server = test_server();
    let resp = server
        .get("/")
        .add_header(
            axum::http::HeaderName::from_static("x-ssl-client-subject-dn"),
            axum::http::HeaderValue::from_static("CN=test"),
        )
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["client_subject"], "CN=test");
}

#[tokio::test]
async fn uplink_valid() {
    let server = test_server();
    let resp = server
        .post("/")
        .json(&serde_json::json!({"id": "dev-1", "current": 42}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["device_id"], "dev-1");
    assert_eq!(body["current"], 42);
    assert_eq!(body["message"], "Uplink received");
}

#[tokio::test]
async fn uplink_empty_body() {
    let server = test_server();
    let resp = server.post("/").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn uplink_invalid_json() {
    let server = test_server();
    let resp = server.post("/").text("{bad json}").await;
    resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
}
