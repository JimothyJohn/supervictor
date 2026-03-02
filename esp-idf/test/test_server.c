#include "unity.h"
#include "network/server.h"
#include <string.h>

// --- parse_request_line ---

TEST_CASE("parse_request_line_get", "[server]") {
    char method[16], path[64];
    server_parse_request_line("GET / HTTP/1.0\r\nHost: 192.168.4.1\r\n\r\n",
                              method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("GET", method);
    TEST_ASSERT_EQUAL_STRING("/", path);
}

TEST_CASE("parse_request_line_get_path", "[server]") {
    char method[16], path[64];
    server_parse_request_line("GET /api/status HTTP/1.0\r\n",
                              method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("GET", method);
    TEST_ASSERT_EQUAL_STRING("/api/status", path);
}

TEST_CASE("parse_request_line_post", "[server]") {
    char method[16], path[64];
    server_parse_request_line("POST /api/configure HTTP/1.0\r\n",
                              method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("POST", method);
    TEST_ASSERT_EQUAL_STRING("/api/configure", path);
}

TEST_CASE("parse_request_line_empty", "[server]") {
    char method[16], path[64];
    server_parse_request_line("", method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("", method);
    TEST_ASSERT_EQUAL_STRING("/", path);
}

TEST_CASE("parse_request_line_method_only", "[server]") {
    char method[16], path[64];
    server_parse_request_line("GET", method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("GET", method);
    TEST_ASSERT_EQUAL_STRING("/", path);
}

TEST_CASE("parse_request_line_wasm_path", "[server]") {
    char method[16], path[64];
    server_parse_request_line("GET /supervictor_portal_bg.wasm HTTP/1.0\r\n",
                              method, sizeof(method), path, sizeof(path));
    TEST_ASSERT_EQUAL_STRING("GET", method);
    TEST_ASSERT_EQUAL_STRING("/supervictor_portal_bg.wasm", path);
}

// --- extract_body ---

TEST_CASE("extract_body_present", "[server]") {
    const char *body = server_extract_body(
        "POST /api/configure HTTP/1.0\r\nContent-Type: application/json\r\n\r\n{\"ssid\":\"Test\"}");
    TEST_ASSERT_EQUAL_STRING("{\"ssid\":\"Test\"}", body);
}

TEST_CASE("extract_body_missing_separator", "[server]") {
    TEST_ASSERT_EQUAL_STRING("", server_extract_body("GET / HTTP/1.0\r\nHost: x"));
}

TEST_CASE("extract_body_empty_body", "[server]") {
    TEST_ASSERT_EQUAL_STRING("", server_extract_body("GET / HTTP/1.0\r\n\r\n"));
}

TEST_CASE("extract_body_multiline", "[server]") {
    const char *body = server_extract_body("POST / HTTP/1.0\r\n\r\nline1\r\nline2");
    TEST_ASSERT_EQUAL_STRING("line1\r\nline2", body);
}

// --- build_response_header ---

TEST_CASE("build_response_header_html", "[server]") {
    char buf[512];
    server_build_response_header(buf, sizeof(buf), "text/html", 914, NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.0 200 OK\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Type: text/html\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Length: 914\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Connection: close\r\n"));
}

TEST_CASE("build_response_header_json", "[server]") {
    char buf[512];
    server_build_response_header(buf, sizeof(buf), "application/json", 64, NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Type: application/json\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Length: 64\r\n"));
}

TEST_CASE("build_response_header_with_extra", "[server]") {
    char buf[512];
    server_build_response_header(buf, sizeof(buf), "application/wasm", 339000,
                                 "Content-Encoding: gzip\r\n");
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Type: application/wasm\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Length: 339000\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Encoding: gzip\r\n"));
}

TEST_CASE("build_response_header_ends_with_blank_line", "[server]") {
    char buf[512];
    int n = server_build_response_header(buf, sizeof(buf), "text/html", 100, NULL);
    TEST_ASSERT_GREATER_THAN(3, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\r\n\r\n"));
}

// --- build_redirect ---

TEST_CASE("build_redirect_location", "[server]") {
    char buf[512];
    server_build_redirect(buf, sizeof(buf), "http://192.168.4.1/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.0 302 Found\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Location: http://192.168.4.1/\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Length: 0\r\n"));
}

TEST_CASE("build_redirect_ends_with_blank_line", "[server]") {
    char buf[512];
    server_build_redirect(buf, sizeof(buf), "http://192.168.4.1/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "\r\n\r\n"));
}

// --- build_error_response ---

TEST_CASE("build_error_response_400", "[server]") {
    char buf[512];
    server_build_error_response(buf, sizeof(buf), 400, "Invalid JSON");
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.0 400 Error\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Type: application/json\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "{\"error\":\"Invalid JSON\"}"));
}

TEST_CASE("build_error_response_content_length", "[server]") {
    char buf[512];
    server_build_error_response(buf, sizeof(buf), 400, "Bad");
    // Body: {"error":"Bad"} = 15 chars
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Length: 15\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "{\"error\":\"Bad\"}"));
}

TEST_CASE("build_error_response_500", "[server]") {
    char buf[512];
    server_build_error_response(buf, sizeof(buf), 500, "Internal");
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.0 500 Error\r\n"));
    TEST_ASSERT_NOT_NULL(strstr(buf, "{\"error\":\"Internal\"}"));
}

// --- build_status_json ---

TEST_CASE("build_status_json_fields", "[server]") {
    char buf[512];
    server_build_status_json(buf, sizeof(buf), "sv-001", "0.1.0", "192.168.4.1", "ap_mode");
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"device_id\":\"sv-001\""));
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"fw_version\":\"0.1.0\""));
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"ip\":\"192.168.4.1\""));
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"state\":\"ap_mode\""));
}

TEST_CASE("build_status_json_valid_json_structure", "[server]") {
    char buf[512];
    server_build_status_json(buf, sizeof(buf), "dev-01", "0.1.0", "10.0.0.1", "sta");
    TEST_ASSERT_EQUAL('{', buf[0]);
    TEST_ASSERT_EQUAL('}', buf[strlen(buf) - 1]);
}

// --- parse_configure_body ---

TEST_CASE("parse_configure_body_valid", "[server]") {
    char ssid[33], password[65];
    int ret = server_parse_configure_body(
        "{\"ssid\":\"TestNet\",\"password\":\"secret123\"}",
        ssid, sizeof(ssid), password, sizeof(password));
    TEST_ASSERT_EQUAL(0, ret);
    TEST_ASSERT_EQUAL_STRING("TestNet", ssid);
    TEST_ASSERT_EQUAL_STRING("secret123", password);
}

TEST_CASE("parse_configure_body_empty_password", "[server]") {
    char ssid[33], password[65];
    int ret = server_parse_configure_body(
        "{\"ssid\":\"OpenNet\",\"password\":\"\"}",
        ssid, sizeof(ssid), password, sizeof(password));
    TEST_ASSERT_EQUAL(0, ret);
    TEST_ASSERT_EQUAL_STRING("OpenNet", ssid);
    TEST_ASSERT_EQUAL_STRING("", password);
}

TEST_CASE("parse_configure_body_missing_ssid", "[server]") {
    char ssid[33], password[65];
    TEST_ASSERT_EQUAL(-1,
        server_parse_configure_body("{\"password\":\"pw\"}", ssid, sizeof(ssid), password, sizeof(password)));
}

TEST_CASE("parse_configure_body_invalid_json", "[server]") {
    char ssid[33], password[65];
    TEST_ASSERT_EQUAL(-1,
        server_parse_configure_body("not json at all", ssid, sizeof(ssid), password, sizeof(password)));
}

TEST_CASE("parse_configure_body_empty", "[server]") {
    char ssid[33], password[65];
    TEST_ASSERT_EQUAL(-1,
        server_parse_configure_body("", ssid, sizeof(ssid), password, sizeof(password)));
}

TEST_CASE("parse_configure_body_extra_fields_ignored", "[server]") {
    char ssid[33], password[65];
    int ret = server_parse_configure_body(
        "{\"ssid\":\"Net\",\"password\":\"pw\",\"extra\":42}",
        ssid, sizeof(ssid), password, sizeof(password));
    TEST_ASSERT_EQUAL(0, ret);
    TEST_ASSERT_EQUAL_STRING("Net", ssid);
}

// --- build_configure_response ---

TEST_CASE("build_configure_response_ok", "[server]") {
    char buf[256];
    server_build_configure_response(buf, sizeof(buf), 1, "Saved");
    TEST_ASSERT_EQUAL_STRING("{\"ok\":true,\"message\":\"Saved\"}", buf);
}

TEST_CASE("build_configure_response_fail", "[server]") {
    char buf[256];
    server_build_configure_response(buf, sizeof(buf), 0, "Failed");
    TEST_ASSERT_EQUAL_STRING("{\"ok\":false,\"message\":\"Failed\"}", buf);
}

// --- format_ip ---

TEST_CASE("format_ip_loopback", "[server]") {
    char buf[16];
    uint8_t ip[] = { 127, 0, 0, 1 };
    server_format_ip(buf, sizeof(buf), ip);
    TEST_ASSERT_EQUAL_STRING("127.0.0.1", buf);
}

TEST_CASE("format_ip_gateway", "[server]") {
    char buf[16];
    uint8_t ip[] = { 192, 168, 4, 1 };
    server_format_ip(buf, sizeof(buf), ip);
    TEST_ASSERT_EQUAL_STRING("192.168.4.1", buf);
}

TEST_CASE("format_ip_zeros", "[server]") {
    char buf[16];
    uint8_t ip[] = { 0, 0, 0, 0 };
    server_format_ip(buf, sizeof(buf), ip);
    TEST_ASSERT_EQUAL_STRING("0.0.0.0", buf);
}

TEST_CASE("format_ip_broadcast", "[server]") {
    char buf[16];
    uint8_t ip[] = { 255, 255, 255, 255 };
    server_format_ip(buf, sizeof(buf), ip);
    TEST_ASSERT_EQUAL_STRING("255.255.255.255", buf);
}

TEST_CASE("format_ip_private", "[server]") {
    char buf[16];
    uint8_t ip[] = { 10, 0, 1, 42 };
    server_format_ip(buf, sizeof(buf), ip);
    TEST_ASSERT_EQUAL_STRING("10.0.1.42", buf);
}
