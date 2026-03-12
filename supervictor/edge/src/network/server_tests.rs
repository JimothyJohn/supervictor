use super::*;

// --- parse_request_line ---

#[test]
fn parse_request_line_get() {
    let (method, path) = parse_request_line("GET / HTTP/1.0\r\nHost: 192.168.4.1\r\n\r\n");
    assert_eq!(method, "GET");
    assert_eq!(path, "/");
}

#[test]
fn parse_request_line_get_path() {
    let (method, path) = parse_request_line("GET /api/status HTTP/1.0\r\n");
    assert_eq!(method, "GET");
    assert_eq!(path, "/api/status");
}

#[test]
fn parse_request_line_post() {
    let (method, path) =
        parse_request_line("POST /api/configure HTTP/1.0\r\nContent-Type: application/json\r\n");
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/configure");
}

#[test]
fn parse_request_line_empty() {
    let (method, path) = parse_request_line("");
    assert_eq!(method, "");
    assert_eq!(path, "/");
}

#[test]
fn parse_request_line_method_only() {
    let (method, path) = parse_request_line("GET");
    assert_eq!(method, "GET");
    assert_eq!(path, "/");
}

#[test]
fn parse_request_line_wasm_path() {
    let (method, path) = parse_request_line("GET /supervictor_portal_bg.wasm HTTP/1.0\r\n");
    assert_eq!(method, "GET");
    assert_eq!(path, "/supervictor_portal_bg.wasm");
}

// --- extract_body ---

#[test]
fn extract_body_present() {
    let request =
        "POST /api/configure HTTP/1.0\r\nContent-Type: application/json\r\n\r\n{\"ssid\":\"Test\"}";
    assert_eq!(extract_body(request), "{\"ssid\":\"Test\"}");
}

#[test]
fn extract_body_missing_separator() {
    assert_eq!(extract_body("GET / HTTP/1.0\r\nHost: x"), "");
}

#[test]
fn extract_body_empty_body() {
    assert_eq!(extract_body("GET / HTTP/1.0\r\n\r\n"), "");
}

#[test]
fn extract_body_multiline() {
    let request = "POST / HTTP/1.0\r\n\r\nline1\r\nline2";
    assert_eq!(extract_body(request), "line1\r\nline2");
}

// --- write_usize ---

#[test]
fn write_usize_zero() {
    let mut s = HString::<8>::new();
    write_usize(&mut s, 0);
    assert_eq!(s.as_str(), "0");
}

#[test]
fn write_usize_single_digit() {
    let mut s = HString::<8>::new();
    write_usize(&mut s, 7);
    assert_eq!(s.as_str(), "7");
}

#[test]
fn write_usize_small() {
    let mut s = HString::<8>::new();
    write_usize(&mut s, 42);
    assert_eq!(s.as_str(), "42");
}

#[test]
fn write_usize_large() {
    let mut s = HString::<8>::new();
    write_usize(&mut s, 772706);
    assert_eq!(s.as_str(), "772706");
}

#[test]
fn write_usize_hundred() {
    let mut s = HString::<8>::new();
    write_usize(&mut s, 100);
    assert_eq!(s.as_str(), "100");
}

#[test]
fn write_usize_appends() {
    let mut s = HString::<16>::new();
    s.push_str("len=").unwrap();
    write_usize(&mut s, 512);
    assert_eq!(s.as_str(), "len=512");
}

// --- build_response_header ---

#[test]
fn build_response_header_html() {
    let h = build_response_header("text/html", 914, None);
    assert!(h.starts_with("HTTP/1.0 200 OK\r\n"));
    assert!(h.contains("Content-Type: text/html\r\n"));
    assert!(h.contains("Content-Length: 914\r\n"));
    assert!(h.contains("Connection: close\r\n"));
    assert!(h.ends_with("\r\n"));
}

#[test]
fn build_response_header_json() {
    let h = build_response_header("application/json", 64, None);
    assert!(h.contains("Content-Type: application/json\r\n"));
    assert!(h.contains("Content-Length: 64\r\n"));
}

#[test]
fn build_response_header_with_extra() {
    let h = build_response_header(
        "application/wasm",
        339000,
        Some("Content-Encoding: gzip\r\n"),
    );
    assert!(h.contains("Content-Type: application/wasm\r\n"));
    assert!(h.contains("Content-Length: 339000\r\n"));
    assert!(h.contains("Content-Encoding: gzip\r\n"));
}

#[test]
fn build_response_header_ends_with_blank_line() {
    let h = build_response_header("text/html", 100, None);
    assert!(h.ends_with("\r\n\r\n"));
}

// --- build_redirect ---

#[test]
fn build_redirect_location() {
    let r = build_redirect("http://192.168.4.1/");
    assert!(r.starts_with("HTTP/1.0 302 Found\r\n"));
    assert!(r.contains("Location: http://192.168.4.1/\r\n"));
    assert!(r.contains("Content-Length: 0\r\n"));
}

#[test]
fn build_redirect_ends_with_blank_line() {
    let r = build_redirect("http://192.168.4.1/");
    assert!(r.ends_with("\r\n\r\n"));
}

// --- build_error_response ---

#[test]
fn build_error_response_400() {
    let r = build_error_response(400, "Invalid JSON");
    assert!(r.starts_with("HTTP/1.0 400 Error\r\n"));
    assert!(r.contains("Content-Type: application/json\r\n"));
    assert!(r.contains("{\"error\":\"Invalid JSON\"}"));
}

#[test]
fn build_error_response_content_length() {
    let r = build_error_response(400, "Bad");
    // Body: {"error":"Bad"} = 15 chars
    assert!(r.contains("Content-Length: 15\r\n"));
    assert!(r.contains("{\"error\":\"Bad\"}"));
}

#[test]
fn build_error_response_500() {
    let r = build_error_response(500, "Internal");
    assert!(r.starts_with("HTTP/1.0 500 Error\r\n"));
    assert!(r.contains("{\"error\":\"Internal\"}"));
}

// --- build_status_json ---

#[test]
fn build_status_json_fields() {
    let json = build_status_json("sv-001", "192.168.4.1", "ap_mode");
    assert!(json.contains("\"device_id\":\"sv-001\""));
    assert!(json.contains("\"fw_version\":\""));
    assert!(json.contains("\"ip\":\"192.168.4.1\""));
    assert!(json.contains("\"state\":\"ap_mode\""));
}

#[test]
fn build_status_json_fw_version() {
    let json = build_status_json("x", "0.0.0.0", "idle");
    // env!("CARGO_PKG_VERSION") is "0.1.0" from Cargo.toml
    assert!(json.contains("\"fw_version\":\"0.1.0\""));
}

#[test]
fn build_status_json_valid_json_structure() {
    let json = build_status_json("dev-01", "10.0.0.1", "sta");
    assert!(json.starts_with('{'));
    assert!(json.ends_with('}'));
}

#[test]
fn build_status_json_roundtrip() {
    // Verify server output can be parsed by serde_json_core (same parser portal's tests use)
    let json = build_status_json("sv-001", "192.168.4.1", "ap_mode");

    #[derive(Deserialize)]
    struct TestStatus<'a> {
        device_id: &'a str,
        fw_version: &'a str,
        ip: &'a str,
        state: &'a str,
    }

    let (status, _): (TestStatus, _) = serde_json_core::from_str(json.as_str()).unwrap();
    assert_eq!(status.device_id, "sv-001");
    assert_eq!(status.fw_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.ip, "192.168.4.1");
    assert_eq!(status.state, "ap_mode");
}

// --- parse_configure_body ---

#[test]
fn parse_configure_body_valid() {
    let (ssid, password) =
        parse_configure_body(r#"{"ssid":"TestNet","password":"secret123"}"#).unwrap();
    assert_eq!(ssid.as_str(), "TestNet");
    assert_eq!(password.as_str(), "secret123");
}

#[test]
fn parse_configure_body_empty_password() {
    let (ssid, password) = parse_configure_body(r#"{"ssid":"OpenNet","password":""}"#).unwrap();
    assert_eq!(ssid.as_str(), "OpenNet");
    assert_eq!(password.as_str(), "");
}

#[test]
fn parse_configure_body_missing_ssid() {
    assert!(parse_configure_body(r#"{"password":"pw"}"#).is_none());
}

#[test]
fn parse_configure_body_invalid_json() {
    assert!(parse_configure_body("not json at all").is_none());
}

#[test]
fn parse_configure_body_empty() {
    assert!(parse_configure_body("").is_none());
}

#[test]
fn parse_configure_body_extra_fields_ignored() {
    let result = parse_configure_body(r#"{"ssid":"Net","password":"pw","extra":42}"#);
    assert!(result.is_some());
    let (ssid, _) = result.unwrap();
    assert_eq!(ssid.as_str(), "Net");
}

#[test]
fn portal_configure_contract() {
    // Simulate what portal/src/api.rs submit_config() sends
    // Portal serializes WifiConfig { ssid: String, password: String } via serde_json
    let portal_json = r#"{"ssid":"MyNetwork","password":"hunter2"}"#;
    let (ssid, password) = parse_configure_body(portal_json).unwrap();
    assert_eq!(ssid.as_str(), "MyNetwork");
    assert_eq!(password.as_str(), "hunter2");
}

// --- build_configure_response ---

#[test]
fn build_configure_response_ok() {
    let r = build_configure_response(true, "Saved");
    assert_eq!(r.as_str(), r#"{"ok":true,"message":"Saved"}"#);
}

#[test]
fn build_configure_response_fail() {
    let r = build_configure_response(false, "Failed");
    assert_eq!(r.as_str(), r#"{"ok":false,"message":"Failed"}"#);
}

#[test]
fn build_configure_response_reboot_message() {
    let r = build_configure_response(true, "Configuration saved. Rebooting...");
    assert!(r.contains("\"ok\":true"));
    assert!(r.contains("Rebooting"));
}

// --- format_ip_octets ---

#[test]
fn format_ip_octets_loopback() {
    assert_eq!(format_ip_octets([127, 0, 0, 1]).as_str(), "127.0.0.1");
}

#[test]
fn format_ip_octets_gateway() {
    assert_eq!(format_ip_octets([192, 168, 4, 1]).as_str(), "192.168.4.1");
}

#[test]
fn format_ip_octets_zeros() {
    assert_eq!(format_ip_octets([0, 0, 0, 0]).as_str(), "0.0.0.0");
}

#[test]
fn format_ip_octets_broadcast() {
    assert_eq!(
        format_ip_octets([255, 255, 255, 255]).as_str(),
        "255.255.255.255"
    );
}

#[test]
fn format_ip_octets_private() {
    assert_eq!(format_ip_octets([10, 0, 1, 42]).as_str(), "10.0.1.42");
}
