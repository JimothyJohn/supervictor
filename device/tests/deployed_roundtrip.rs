//! Integration tests against a deployed HTTPS endpoint (API Gateway).
//!
//! Validates that the device's JSON payloads are accepted by the live Lambda
//! behind HTTPS. Uses reqwest + tokio from [dev-dependencies].
//!
//! Requires:
//!   - DEPLOYED_URL env var (e.g. "https://abc.execute-api.us-east-1.amazonaws.com/dev")
//!   - Skips gracefully when DEPLOYED_URL is unset
//!
//! Run:
//!   DEPLOYED_URL=https://... cargo test --test deployed_roundtrip \
//!     --target aarch64-apple-darwin

fn deployed_url() -> Option<String> {
    std::env::var("DEPLOYED_URL").ok()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("supervictor/0.1.0 (deployed_roundtrip)")
        .build()
        .expect("build reqwest client")
}

#[tokio::test]
async fn deployed_get_root() {
    let base = match deployed_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: DEPLOYED_URL not set");
            return;
        }
    };

    let resp = client()
        .get(format!("{}/", base.trim_end_matches('/')))
        .send()
        .await
        .expect("GET deployed endpoint");

    assert_eq!(resp.status(), 200, "GET / should return 200");
    let body = resp.text().await.expect("read body");
    assert!(body.contains("message"), "body missing 'message' key: {body}");
}

#[tokio::test]
async fn deployed_post_uplink() {
    let base = match deployed_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: DEPLOYED_URL not set");
            return;
        }
    };

    let json = r#"{"id":"deployed-test","current":42}"#;
    let resp = client()
        .post(format!("{}/", base.trim_end_matches('/')))
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .expect("POST deployed endpoint");

    assert_eq!(resp.status(), 200, "POST / should return 200");
    let body = resp.text().await.expect("read body");
    assert!(body.contains("device_id"), "body missing 'device_id': {body}");
    assert!(
        body.contains("deployed-test"),
        "body should echo device id: {body}"
    );
}

#[tokio::test]
async fn deployed_post_boundary_current() {
    let base = match deployed_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: DEPLOYED_URL not set");
            return;
        }
    };

    let json = format!(r#"{{"id":"i32-max-deployed","current":{}}}"#, i32::MAX);
    let resp = client()
        .post(format!("{}/", base.trim_end_matches('/')))
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .expect("POST with i32::MAX");

    assert_eq!(resp.status(), 200, "POST with i32::MAX should return 200");
    let body = resp.text().await.expect("read body");
    assert!(!body.is_empty(), "body should not be empty");
}

#[tokio::test]
async fn deployed_post_missing_body() {
    let base = match deployed_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: DEPLOYED_URL not set");
            return;
        }
    };

    let resp = client()
        .post(format!("{}/", base.trim_end_matches('/')))
        .send()
        .await
        .expect("POST with no body");

    assert_eq!(resp.status(), 400, "POST without body should return 400");
}
