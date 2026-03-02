#include "unity.h"
#include "network/dns.h"
#include <string.h>

static const uint8_t GATEWAY[4] = { 192, 168, 4, 1 };

// Minimal valid DNS A query for "example.com"
static const uint8_t EXAMPLE_COM_QUERY[29] = {
    0xAB, 0xCD, // Transaction ID
    0x01, 0x00, // Flags: standard query
    0x00, 0x01, // Questions: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // QNAME: example.com
    0x07, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 0x03, 'c', 'o', 'm',
    0x00, // End of QNAME
    0x00, 0x01, // QTYPE: A
    0x00, 0x01, // QCLASS: IN
};

// --- find_qname_end ---

TEST_CASE("find_qname_end_single_label", "[dns]") {
    uint8_t data[] = { 0x05, 'h', 'e', 'l', 'l', 'o', 0x00 };
    TEST_ASSERT_EQUAL(7, dns_find_qname_end(data, sizeof(data), 0));
}

TEST_CASE("find_qname_end_multi_label", "[dns]") {
    uint8_t data[] = {
        0x03, 'w', 'w', 'w', 0x06, 'g', 'o', 'o', 'g', 'l', 'e',
        0x03, 'c', 'o', 'm', 0x00,
    };
    TEST_ASSERT_EQUAL(16, dns_find_qname_end(data, sizeof(data), 0));
}

TEST_CASE("find_qname_end_with_offset", "[dns]") {
    TEST_ASSERT_EQUAL(25,
        dns_find_qname_end(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY), 12));
}

TEST_CASE("find_qname_end_empty_buffer", "[dns]") {
    TEST_ASSERT_EQUAL(-1, dns_find_qname_end(NULL, 0, 0));
}

TEST_CASE("find_qname_end_truncated", "[dns]") {
    uint8_t data[] = { 0x05, 'h', 'e' };
    TEST_ASSERT_EQUAL(-1, dns_find_qname_end(data, sizeof(data), 0));
}

TEST_CASE("find_qname_end_root", "[dns]") {
    uint8_t data[] = { 0x00 };
    TEST_ASSERT_EQUAL(1, dns_find_qname_end(data, sizeof(data), 0));
}

TEST_CASE("find_qname_end_pointer", "[dns]") {
    uint8_t data[] = { 0xC0, 0x0C };
    TEST_ASSERT_EQUAL(2, dns_find_qname_end(data, sizeof(data), 0));
}

// --- build_dns_response ---

TEST_CASE("build_dns_response_valid_query", "[dns]") {
    uint8_t resp[512];
    int n = dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                                resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_GREATER_THAN(12, n);
}

TEST_CASE("build_dns_response_preserves_txn_id", "[dns]") {
    uint8_t resp[512];
    dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                       resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_EQUAL_HEX8(0xAB, resp[0]);
    TEST_ASSERT_EQUAL_HEX8(0xCD, resp[1]);
}

TEST_CASE("build_dns_response_flags", "[dns]") {
    uint8_t resp[512];
    dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                       resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_EQUAL_HEX8(0x81, resp[2]);
    TEST_ASSERT_EQUAL_HEX8(0x80, resp[3]);
}

TEST_CASE("build_dns_response_answer_count", "[dns]") {
    uint8_t resp[512];
    dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                       resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_EQUAL_HEX8(0x00, resp[6]);
    TEST_ASSERT_EQUAL_HEX8(0x01, resp[7]);
}

TEST_CASE("build_dns_response_gateway_ip", "[dns]") {
    uint8_t resp[512];
    int n = dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                                resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_EQUAL_UINT8(192, resp[n - 4]);
    TEST_ASSERT_EQUAL_UINT8(168, resp[n - 3]);
    TEST_ASSERT_EQUAL_UINT8(4, resp[n - 2]);
    TEST_ASSERT_EQUAL_UINT8(1, resp[n - 1]);
}

TEST_CASE("build_dns_response_too_short", "[dns]") {
    uint8_t query[16] = {0};
    uint8_t resp[512];
    TEST_ASSERT_EQUAL(-1,
        dns_build_response(query, sizeof(query), resp, sizeof(resp), GATEWAY));
}

TEST_CASE("build_dns_response_question_copied", "[dns]") {
    uint8_t resp[512];
    dns_build_response(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY),
                       resp, sizeof(resp), GATEWAY);
    // Question starts at offset 12 in both query and response
    int qname_end = dns_find_qname_end(EXAMPLE_COM_QUERY, sizeof(EXAMPLE_COM_QUERY), 12);
    size_t question_len = (size_t)qname_end + 4 - 12;
    TEST_ASSERT_EQUAL_MEMORY(EXAMPLE_COM_QUERY + 12, resp + 12, question_len);
}

// Android captive portal probe
static const uint8_t ANDROID_PROBE[47] = {
    0x12, 0x34,
    0x01, 0x00,
    0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    17, 'c', 'o', 'n', 'n', 'e', 'c', 't', 'i', 'v', 'i', 't', 'y', 'c', 'h',
    'e', 'c', 'k', 7, 'g', 's', 't', 'a', 't', 'i', 'c', 3, 'c', 'o', 'm',
    0x00,
    0x00, 0x01,
    0x00, 0x01,
};

TEST_CASE("build_dns_response_android_captive_portal", "[dns]") {
    uint8_t resp[512];
    int n = dns_build_response(ANDROID_PROBE, sizeof(ANDROID_PROBE),
                                resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_EQUAL_HEX8(0x12, resp[0]);
    TEST_ASSERT_EQUAL_HEX8(0x34, resp[1]);
    TEST_ASSERT_EQUAL_UINT8(192, resp[n - 4]);
    TEST_ASSERT_EQUAL_UINT8(168, resp[n - 3]);
}

// iOS captive portal probe
static const uint8_t IOS_PROBE[35] = {
    0x56, 0x78,
    0x01, 0x00,
    0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    7, 'c', 'a', 'p', 't', 'i', 'v', 'e', 5, 'a', 'p', 'p', 'l', 'e', 3, 'c',
    'o', 'm', 0x00,
    0x00, 0x01,
    0x00, 0x01,
};

TEST_CASE("build_dns_response_ios_captive_portal", "[dns]") {
    uint8_t resp[512];
    int n = dns_build_response(IOS_PROBE, sizeof(IOS_PROBE),
                                resp, sizeof(resp), GATEWAY);
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_EQUAL_HEX8(0x56, resp[0]);
    TEST_ASSERT_EQUAL_HEX8(0x78, resp[1]);
    TEST_ASSERT_EQUAL_UINT8(192, resp[n - 4]);
}

TEST_CASE("build_dns_response_different_txn_ids", "[dns]") {
    uint8_t q1[29], q2[29];
    memcpy(q1, EXAMPLE_COM_QUERY, 29);
    memcpy(q2, EXAMPLE_COM_QUERY, 29);
    q1[0] = 0x00; q1[1] = 0x01;
    q2[0] = 0xFF; q2[1] = 0xFE;

    uint8_t r1[512], r2[512];
    dns_build_response(q1, 29, r1, 512, GATEWAY);
    dns_build_response(q2, 29, r2, 512, GATEWAY);

    TEST_ASSERT_EQUAL_HEX8(0x00, r1[0]);
    TEST_ASSERT_EQUAL_HEX8(0x01, r1[1]);
    TEST_ASSERT_EQUAL_HEX8(0xFF, r2[0]);
    TEST_ASSERT_EQUAL_HEX8(0xFE, r2[1]);
}
