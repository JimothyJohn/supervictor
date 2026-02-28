//! Integration tests using real TCP loopback with mock servers.
//! Simulates the full request/response cycle the device performs against AWS.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use supervictor::models::uplink::UplinkMessage;
use supervictor::network::http::{get_request, parse_response, post_request};

// ── Mock server infrastructure ───────────────────────────────

/// Canned AWS API Gateway 200 response for GET /hello
const MOCK_GET_RESPONSE: &str = concat!(
    "HTTP/1.1 200 OK\r\n",
    "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n",
    "Content-Type: application/json\r\n",
    "Content-Length: 36\r\n",
    "x-amzn-RequestId: get-req-id-001\r\n",
    "x-amz-apigw-id: GetGatewayId001\r\n",
    "X-Amzn-Trace-Id: Root=1-get-trace-001\r\n",
    "\r\n",
    r#"{"message":"Hello from Supervictor!"}"#,
);

/// Canned AWS API Gateway 200 response for POST /hello
const MOCK_POST_RESPONSE: &str = concat!(
    "HTTP/1.1 200 OK\r\n",
    "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n",
    "Content-Type: application/json\r\n",
    "Content-Length: 28\r\n",
    "x-amzn-RequestId: post-req-id-001\r\n",
    "x-amz-apigw-id: PostGatewayId001\r\n",
    "X-Amzn-Trace-Id: Root=1-post-trace-001\r\n",
    "\r\n",
    r#"{"message":"Uplink received"}"#,
);

/// Spawns a one-shot TCP server that accepts a single connection,
/// reads the request, sends back a canned response, and returns
/// the received bytes via a JoinHandle.
fn one_shot_server(response: &'static str) -> (String, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let addr_str = format!("127.0.0.1:{}", addr.port());

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        buf.truncate(n);

        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        // Shutdown write side to signal EOF
        let _ = stream.shutdown(std::net::Shutdown::Write);

        buf
    });

    (addr_str, handle)
}

/// Connects to a server, sends a request, reads the full response.
fn send_and_receive(addr: &str, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();

    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    // Shutdown write side so server knows we're done sending
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

// ── Tests ────────────────────────────────────────────────────

#[test]
fn get_roundtrip_through_mock_server() {
    let (addr, server_handle) = one_shot_server(MOCK_GET_RESPONSE);

    let host = addr.as_str();
    let request = get_request(host, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str());

    let parsed = parse_response(&response_str).expect("parse_response failed");
    assert_eq!(parsed.x_amzn_request_id.as_str(), "get-req-id-001");
    assert!(parsed.body.contains("Hello from Supervictor!"));
    assert_eq!(parsed.content_type.as_str(), "application/json");

    // Verify server received a valid GET request
    let received = server_handle.join().unwrap();
    let received_str = String::from_utf8(received).unwrap();
    assert!(received_str.starts_with("GET /hello HTTP/1.0"));
    assert!(received_str.contains(&format!("Host: {}", host)));
}

#[test]
fn post_roundtrip_through_mock_server() {
    let (addr, server_handle) = one_shot_server(MOCK_POST_RESPONSE);

    let host = addr.as_str();
    let msg = UplinkMessage {
        id: "device-001".try_into().unwrap(),
        current: 42,
    };
    let request = post_request(host, &msg, Some("/hello"));
    let response_str = send_and_receive(&addr, request.as_str());

    let parsed = parse_response(&response_str).expect("parse_response failed");
    assert_eq!(parsed.x_amzn_request_id.as_str(), "post-req-id-001");
    assert!(parsed.body.contains("Uplink received"));

    // Verify server received valid POST with correct JSON body
    let received = server_handle.join().unwrap();
    let received_str = String::from_utf8(received).unwrap();
    assert!(received_str.starts_with("POST /hello HTTP/1.1"));
    assert!(received_str.contains(r#"{"id":"device-001","current":42}"#));
}

#[test]
fn post_content_length_matches_actual_body_on_wire() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());

    let addr_clone = addr.clone();
    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        // Extract Content-Length
        let cl_line = request
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-length:"))
            .expect("missing Content-Length header");
        let claimed: usize = cl_line.split(':').nth(1).unwrap().trim().parse().unwrap();

        // Extract actual body (after \r\n\r\n)
        let body_start = request.find("\r\n\r\n").unwrap() + 4;
        let actual_body_len = request[body_start..].len();

        assert_eq!(
            claimed, actual_body_len,
            "Content-Length {} != actual body length {}",
            claimed, actual_body_len
        );

        stream.write_all(MOCK_POST_RESPONSE.as_bytes()).unwrap();
        stream.flush().unwrap();
        let _ = stream.shutdown(std::net::Shutdown::Write);
    });

    let msg = UplinkMessage {
        id: "cl-verify".try_into().unwrap(),
        current: 12345,
    };
    let request = post_request(&addr_clone, &msg, Some("/hello"));
    let _ = send_and_receive(&addr, request.as_str());
    server_handle.join().unwrap();
}

#[test]
fn server_receives_correct_host_header() {
    let (addr, server_handle) = one_shot_server(MOCK_GET_RESPONSE);

    let request = get_request(&addr, Some("/"));
    let _ = send_and_receive(&addr, request.as_str());

    let received = server_handle.join().unwrap();
    let received_str = String::from_utf8(received).unwrap();
    assert!(
        received_str.contains(&format!("Host: {}", addr)),
        "Host header missing or wrong in: {}",
        received_str
    );
}

#[test]
fn parallel_servers_no_cross_contamination() {
    let (addr1, server1) = one_shot_server(MOCK_GET_RESPONSE);
    let (addr2, server2) = one_shot_server(MOCK_POST_RESPONSE);

    let req1 = get_request(&addr1, Some("/first"));
    let req2 = get_request(&addr2, Some("/second"));

    let resp1 = send_and_receive(&addr1, req1.as_str());
    let resp2 = send_and_receive(&addr2, req2.as_str());

    let parsed1 = parse_response(&resp1).unwrap();
    let parsed2 = parse_response(&resp2).unwrap();

    // Server 1 returned GET response, server 2 returned POST response
    assert!(parsed1.body.contains("Hello from Supervictor!"));
    assert!(parsed2.body.contains("Uplink received"));

    // Verify each server got the right request
    let recv1 = String::from_utf8(server1.join().unwrap()).unwrap();
    let recv2 = String::from_utf8(server2.join().unwrap()).unwrap();
    assert!(recv1.contains("/first"));
    assert!(recv2.contains("/second"));
}

#[test]
fn large_body_near_capacity() {
    // Create a response with body just under the 1024-byte HString limit
    let body = "X".repeat(1020);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    // Leak to get 'static lifetime for the one_shot_server
    let response: &'static str = Box::leak(response.into_boxed_str());

    let (addr, _server) = one_shot_server(response);
    let req = get_request(&addr, Some("/"));
    let resp_str = send_and_receive(&addr, req.as_str());
    let parsed = parse_response(&resp_str).unwrap();
    assert_eq!(parsed.body.len(), 1020);
}

#[test]
fn server_closes_immediately_returns_error_or_empty() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());

    // Server that accepts and immediately closes
    let _server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream); // Close immediately
    });

    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();

    let req = get_request(&addr, Some("/"));
    let _ = stream.write_all(req.as_str().as_bytes());

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    // Empty response should be treated as an error by parse_response
    if !response.is_empty() {
        let _ = parse_response(&response);
    } else {
        assert!(parse_response("").is_err());
    }
}
