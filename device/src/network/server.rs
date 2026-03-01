//! HTTP server for the device portal.
//!
//! Pure functions (parser, builders) compile on all targets.
//! The async `serve` task is gated behind `#[cfg(feature = "portal")]`.

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
    h.push_str("\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").ok();
    h
}

/// Build an HTTP error response with a JSON body.
pub fn build_error_response(status: u16, message: &str) -> HString<256> {
    let mut h = HString::<256>::new();
    h.push_str("HTTP/1.0 ").ok();
    write_usize(&mut h, status as usize);
    h.push_str(" Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: ").ok();
    // Body: {"error":"<message>"}
    let body_len = 10 + message.len() + 2;
    write_usize(&mut h, body_len);
    h.push_str("\r\n\r\n{\"error\":\"").ok();
    h.push_str(message).ok();
    h.push_str("\"}").ok();
    h
}

// --- API endpoints ---

/// Build the JSON body for GET /api/status.
/// Fields match portal/src/models.rs DeviceStatus.
pub fn build_status_json(device_id: &str, ip: &str, state: &str) -> HString<256> {
    let mut body = HString::<256>::new();
    body.push_str("{\"device_id\":\"").ok();
    body.push_str(device_id).ok();
    body.push_str("\",\"fw_version\":\"").ok();
    body.push_str(env!("CARGO_PKG_VERSION")).ok();
    body.push_str("\",\"ip\":\"").ok();
    body.push_str(ip).ok();
    body.push_str("\",\"state\":\"").ok();
    body.push_str(state).ok();
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
    body.push_str(message).ok();
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

// --- Embedded portal server ---

#[cfg(feature = "portal")]
const INDEX_HTML: &[u8] = include_bytes!("../../../portal/dist/index.html");
#[cfg(feature = "portal")]
const PORTAL_JS: &[u8] = include_bytes!("../../../portal/dist/supervictor_portal.js");
#[cfg(feature = "portal")]
const PORTAL_WASM_GZ: &[u8] =
    include_bytes!("../../../portal/dist/supervictor_portal_bg.wasm.gz");

#[cfg(feature = "portal")]
#[embassy_executor::task]
pub async fn serve(stack: embassy_net::Stack<'static>) {
    use crate::config::{
        AP_GATEWAY, DEVICE_ID, PORTAL_PORT, PORTAL_RX_BUFFER_SIZE, PORTAL_TIMEOUT,
        PORTAL_TX_BUFFER_SIZE,
    };
    use embassy_net::tcp::TcpSocket;
    use embedded_io_async::Write;
    use esp_println::println;

    // Wait for network link
    loop {
        if stack.is_link_up() {
            break;
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;
    }
    println!("Portal server starting on port {}", PORTAL_PORT);

    let ip = format_ip_octets(AP_GATEWAY);

    loop {
        let mut rx_buf = [0u8; PORTAL_RX_BUFFER_SIZE];
        let mut tx_buf = [0u8; PORTAL_TX_BUFFER_SIZE];
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(PORTAL_TIMEOUT));

        if socket.accept(PORTAL_PORT).await.is_err() {
            continue;
        }

        let mut req_buf = [0u8; 512];
        let n = match embedded_io_async::Read::read(&mut socket, &mut req_buf).await {
            Ok(0) | Err(_) => {
                socket.close();
                continue;
            }
            Ok(n) => n,
        };

        let request = core::str::from_utf8(&req_buf[..n]).unwrap_or("");
        let (method, path) = parse_request_line(request);

        match (method, path) {
            ("GET", "/") | ("GET", "/index.html") => {
                write_chunked(&mut socket, "text/html", INDEX_HTML, None).await;
            }
            ("GET", p) if p.ends_with(".js") => {
                write_chunked(&mut socket, "application/javascript", PORTAL_JS, None).await;
            }
            ("GET", p) if p.ends_with(".wasm") => {
                write_chunked(
                    &mut socket,
                    "application/wasm",
                    PORTAL_WASM_GZ,
                    Some("Content-Encoding: gzip\r\n"),
                )
                .await;
            }
            ("GET", "/api/status") => {
                let body = build_status_json(DEVICE_ID, ip.as_str(), "ap_mode");
                let header = build_response_header("application/json", body.len(), None);
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
            }
            ("POST", "/api/configure") => {
                let body_str = extract_body(request);
                match parse_configure_body(body_str) {
                    Some((ssid, _password)) => {
                        // TODO: write to NVS when TODO #2 (NVS) is done
                        println!("WiFi config received: ssid={}", ssid.as_str());
                        let resp = build_configure_response(
                            true,
                            "Configuration saved. Rebooting...",
                        );
                        let header =
                            build_response_header("application/json", resp.len(), None);
                        let _ = socket.write_all(header.as_bytes()).await;
                        let _ = socket.write_all(resp.as_bytes()).await;
                    }
                    None => {
                        let resp = build_error_response(400, "Invalid JSON");
                        let _ = socket.write_all(resp.as_bytes()).await;
                    }
                }
            }
            _ => {
                // Captive portal: redirect everything else to /
                let resp = build_redirect("http://192.168.4.1/");
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        }

        socket.close();
    }
}

/// Stream a byte slice to the socket with HTTP headers, in 2 KB chunks.
#[cfg(feature = "portal")]
async fn write_chunked(
    socket: &mut embassy_net::tcp::TcpSocket<'_>,
    content_type: &str,
    data: &[u8],
    extra_headers: Option<&str>,
) {
    use embedded_io_async::Write;
    let header = build_response_header(content_type, data.len(), extra_headers);
    if socket.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    for chunk in data.chunks(2048) {
        if socket.write_all(chunk).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
