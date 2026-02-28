//! Integration tests against a running SAM local endpoint.
//! These tests are gated on the SAM_LOCAL_URL environment variable.
//! Run with: SAM_LOCAL_URL=http://127.0.0.1:3000 cargo test --test sam_local_roundtrip

use std::io::{Read, Write};
use std::net::TcpStream;

use supervictor::models::uplink::UplinkMessage;
use supervictor::network::http::{get_request, parse_response, post_request};

// ── Helpers ──────────────────────────────────────────────────

/// Returns SAM_LOCAL_URL if set, otherwise None (test should skip).
fn sam_url() -> Option<String> {
    std::env::var("SAM_LOCAL_URL").ok()
}

/// Parses "http://host:port" into (connect_addr, host_header).
/// Returns ("127.0.0.1:3000", "127.0.0.1:3000") for typical SAM local.
fn parse_url(url: &str) -> (String, String) {
    let stripped = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let addr = stripped.trim_end_matches('/');
    (addr.to_string(), addr.to_string())
}

/// Sends raw HTTP request bytes and reads the full response.
fn send_and_receive(addr: &str, request: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    stream.write_all(request).unwrap();
    stream.flush().unwrap();
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

// ── Tests ────────────────────────────────────────────────────

#[test]
fn sam_local_get_hello() {
    let url = match sam_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: SAM_LOCAL_URL not set");
            return;
        }
    };
    let (addr, host) = parse_url(&url);
    let request = get_request(&host, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str().as_bytes());
    let parsed = parse_response(&response_str).expect("parse_response failed on SAM local GET");
    assert!(
        !parsed.body.is_empty(),
        "SAM local GET /hello returned empty body"
    );
}

#[test]
fn sam_local_post_hello() {
    let url = match sam_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: SAM_LOCAL_URL not set");
            return;
        }
    };
    let (addr, host) = parse_url(&url);
    let msg = UplinkMessage {
        id: "sam-test-device".try_into().unwrap(),
        current: 42,
    };
    let request = post_request(&host, &msg, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str().as_bytes());
    let parsed = parse_response(&response_str).expect("parse_response failed on SAM local POST");
    assert!(
        !parsed.body.is_empty(),
        "SAM local POST /hello returned empty body"
    );
}

#[test]
fn sam_local_post_i32_max_current() {
    let url = match sam_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: SAM_LOCAL_URL not set");
            return;
        }
    };
    let (addr, host) = parse_url(&url);
    let msg = UplinkMessage {
        id: "i32-max-test".try_into().unwrap(),
        current: i32::MAX,
    };
    let request = post_request(&host, &msg, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str().as_bytes());
    let parsed = parse_response(&response_str).expect("parse_response failed on i32::MAX POST");
    assert!(
        !parsed.body.is_empty(),
        "SAM local did not handle i32::MAX"
    );
}

#[test]
fn sam_local_get_nonexistent_path() {
    let url = match sam_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: SAM_LOCAL_URL not set");
            return;
        }
    };
    let (addr, host) = parse_url(&url);
    let request = get_request(&host, Some("/nonexistent"));
    let response_str = send_and_receive(&addr, request.as_str().as_bytes());
    // Should get some kind of error response but parse_response should still handle it
    let parsed = parse_response(&response_str).expect("parse_response failed on 404");
    assert!(
        !parsed.body.is_empty(),
        "SAM local should return error body for nonexistent path"
    );
}

#[test]
fn sam_local_post_empty_id() {
    let url = match sam_url() {
        Some(u) => u,
        None => {
            eprintln!("SKIP: SAM_LOCAL_URL not set");
            return;
        }
    };
    let (addr, host) = parse_url(&url);
    let msg = UplinkMessage {
        id: heapless::String::new(),
        current: 0,
    };
    let request = post_request(&host, &msg, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str().as_bytes());
    // Server may accept or reject empty id — either way parse_response should work
    let parsed = parse_response(&response_str).expect("parse_response failed on empty id POST");
    let _ = parsed.body;
}
