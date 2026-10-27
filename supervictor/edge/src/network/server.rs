//! HTTP request parsing and response building for a device-side server.
//!
//! Pure functions only, compiled and tested on every target. The captive
//! portal task that used them (feature `portal`) was dropped 2026-10-27:
//! its wasm bundle was never in the tree, so the feature could not build.
//! These stay as the building blocks for whatever local UI returns.

use heapless::String as HString;
use serde::Deserialize;

// --- Request parsing ---

/// Extract HTTP method and path from the first request line.
/// "GET /api/status HTTP/1.0\r\n..." → ("GET", "/api/status")
pub fn parse_request_line(request: &str) -> (&str, &str) {
    let first_line = request.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    (method, path)
}

/// Extract the body after the \r\n\r\n header separator.
pub fn extract_body(request: &str) -> &str {
    request
        .find("\r\n\r\n")
        .map(|i| &request[i + 4..])
        .unwrap_or("")
}

// --- JSON helpers ---

/// Push `value` into `s`, escaping `"` and `\` for safe JSON string embedding.
fn push_json_str<const N: usize>(s: &mut HString<N>, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => {
                s.push('\\').ok();
                s.push('"').ok();
            }
            '\\' => {
                s.push('\\').ok();
                s.push('\\').ok();
            }
            _ => {
                s.push(ch).ok();
            }
        }
    }
}

// --- Response building ---

/// Write a usize as decimal digits into an HString.
pub fn write_usize<const N: usize>(s: &mut HString<N>, mut n: usize) {
    if n == 0 {
        s.push('0').ok();
        return;
    }
    let mut buf = [0u8; 7]; // max 7 digits (up to 9999999)
    let mut i = 0;
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char).ok();
    }
}

/// Build an HTTP 200 response header.
pub fn build_response_header(
    content_type: &str,
    content_length: usize,
    extra_headers: Option<&str>,
) -> HString<256> {
    let mut h = HString::<256>::new();
    h.push_str("HTTP/1.0 200 OK\r\nContent-Type: ").ok();
    h.push_str(content_type).ok();
    h.push_str("\r\nContent-Length: ").ok();
    write_usize(&mut h, content_length);
    h.push_str("\r\nConnection: close\r\n").ok();
    if let Some(extra) = extra_headers {
        h.push_str(extra).ok();
    }
    h.push_str("\r\n").ok();
    h
}

/// Build an HTTP 302 redirect response.
pub fn build_redirect(location: &str) -> HString<256> {
    let mut h = HString::<256>::new();
    h.push_str("HTTP/1.0 302 Found\r\nLocation: ").ok();
    h.push_str(location).ok();
    h.push_str("\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .ok();
    h
}

/// Build an HTTP error response with a JSON body.
pub fn build_error_response(status: u16, message: &str) -> HString<256> {
    // Build body first so Content-Length accounts for escaped chars
    let mut body = HString::<128>::new();
    body.push_str("{\"error\":\"").ok();
    push_json_str(&mut body, message);
    body.push_str("\"}").ok();

    let mut h = HString::<256>::new();
    h.push_str("HTTP/1.0 ").ok();
    write_usize(&mut h, status as usize);
    h.push_str(" Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: ")
        .ok();
    write_usize(&mut h, body.len());
    h.push_str("\r\n\r\n").ok();
    h.push_str(body.as_str()).ok();
    h
}

// --- API endpoints ---

/// Build the JSON body for GET /api/status.
/// Fields match portal/src/models.rs DeviceStatus.
pub fn build_status_json(device_id: &str, ip: &str, state: &str) -> HString<256> {
    let mut body = HString::<256>::new();
    body.push_str("{\"device_id\":\"").ok();
    push_json_str(&mut body, device_id);
    body.push_str("\",\"fw_version\":\"").ok();
    push_json_str(&mut body, env!("CARGO_PKG_VERSION"));
    body.push_str("\",\"ip\":\"").ok();
    push_json_str(&mut body, ip);
    body.push_str("\",\"state\":\"").ok();
    push_json_str(&mut body, state);
    body.push_str("\"}").ok();
    body
}

/// WiFi configuration received from POST /api/configure.
/// Matches portal/src/models.rs WifiConfig (Serialize side).
#[derive(Debug, Deserialize)]
struct WifiConfigRequest {
    ssid: HString<32>,
    password: HString<64>,
}

/// Parse the configure request body. Returns None on invalid JSON.
pub fn parse_configure_body(body: &str) -> Option<(HString<32>, HString<64>)> {
    let (config, _): (WifiConfigRequest, _) = serde_json_core::from_str(body).ok()?;
    Some((config.ssid, config.password))
}

/// Build the JSON response for POST /api/configure.
pub fn build_configure_response(ok: bool, message: &str) -> HString<256> {
    let mut body = HString::<256>::new();
    body.push_str("{\"ok\":").ok();
    body.push_str(if ok { "true" } else { "false" }).ok();
    body.push_str(",\"message\":\"").ok();
    push_json_str(&mut body, message);
    body.push_str("\"}").ok();
    body
}

// --- IP formatting ---

/// Format IPv4 octets as a dotted-decimal string.
pub fn format_ip_octets(octets: [u8; 4]) -> HString<16> {
    let mut s = HString::<16>::new();
    for (i, octet) in octets.iter().enumerate() {
        if i > 0 {
            s.push('.').ok();
        }
        write_usize(&mut s, *octet as usize);
    }
    s
}
