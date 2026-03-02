#include "unity.h"
#include "models/uplink.h"
#include <string.h>
#include <limits.h>

// ── Serialization ──

TEST_CASE("uplink_serialize_basic", "[uplink]") {
    uplink_message_t msg = { .id = "dev-1", .current = 42 };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"id\":\"dev-1\""));
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"current\":42"));
}

TEST_CASE("uplink_serialize_empty_id", "[uplink]") {
    uplink_message_t msg = { .id = "", .current = 0 };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"id\":\"\""));
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"current\":0"));
}

TEST_CASE("uplink_serialize_negative_current", "[uplink]") {
    uplink_message_t msg = { .id = "x", .current = -1 };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"current\":-1"));
}

TEST_CASE("uplink_serialize_zero_current", "[uplink]") {
    uplink_message_t msg = { .id = "z", .current = 0 };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "\"current\":0"));
}

TEST_CASE("uplink_serialize_i32_max", "[uplink]") {
    uplink_message_t msg = { .id = "x", .current = INT32_MAX };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "2147483647"));
}

TEST_CASE("uplink_serialize_i32_min", "[uplink]") {
    uplink_message_t msg = { .id = "x", .current = INT32_MIN };
    char buf[256];
    int n = uplink_to_json(&msg, buf, sizeof(buf));
    TEST_ASSERT_GREATER_THAN(0, n);
    TEST_ASSERT_NOT_NULL(strstr(buf, "-2147483648"));
}

// ── Deserialization ──

TEST_CASE("uplink_deserialize_basic", "[uplink]") {
    uplink_message_t msg;
    sv_error_t err = uplink_from_json("{\"id\":\"dev-1\",\"current\":42}", &msg);
    TEST_ASSERT_EQUAL(SV_OK, err);
    TEST_ASSERT_EQUAL_STRING("dev-1", msg.id);
    TEST_ASSERT_EQUAL_INT32(42, msg.current);
}

TEST_CASE("uplink_deserialize_reordered_fields", "[uplink]") {
    uplink_message_t msg;
    sv_error_t err = uplink_from_json("{\"current\":99,\"id\":\"reorder\"}", &msg);
    TEST_ASSERT_EQUAL(SV_OK, err);
    TEST_ASSERT_EQUAL_STRING("reorder", msg.id);
    TEST_ASSERT_EQUAL_INT32(99, msg.current);
}

TEST_CASE("uplink_deserialize_negative", "[uplink]") {
    uplink_message_t msg;
    sv_error_t err = uplink_from_json("{\"id\":\"neg\",\"current\":-500}", &msg);
    TEST_ASSERT_EQUAL(SV_OK, err);
    TEST_ASSERT_EQUAL_INT32(-500, msg.current);
}

TEST_CASE("uplink_deserialize_id_at_64_chars", "[uplink]") {
    // Build JSON with 64-char id
    char json[256];
    char long_id[65];
    memset(long_id, 'A', 64);
    long_id[64] = '\0';
    snprintf(json, sizeof(json), "{\"id\":\"%s\",\"current\":0}", long_id);

    uplink_message_t msg;
    sv_error_t err = uplink_from_json(json, &msg);
    TEST_ASSERT_EQUAL(SV_OK, err);
    TEST_ASSERT_EQUAL(64, strlen(msg.id));
}

TEST_CASE("uplink_deserialize_id_over_64_fails", "[uplink]") {
    char json[256];
    char long_id[66];
    memset(long_id, 'A', 65);
    long_id[65] = '\0';
    snprintf(json, sizeof(json), "{\"id\":\"%s\",\"current\":0}", long_id);

    uplink_message_t msg;
    sv_error_t err = uplink_from_json(json, &msg);
    TEST_ASSERT_EQUAL(SV_ERR_DESERIALIZATION, err);
}

// ── Roundtrip ──

TEST_CASE("uplink_roundtrip_preserves_data", "[uplink]") {
    uplink_message_t original = { .id = "roundtrip-test", .current = -999 };
    char json[256];
    int n = uplink_to_json(&original, json, sizeof(json));
    TEST_ASSERT_GREATER_THAN(0, n);

    uplink_message_t recovered;
    sv_error_t err = uplink_from_json(json, &recovered);
    TEST_ASSERT_EQUAL(SV_OK, err);
    TEST_ASSERT_EQUAL_STRING(original.id, recovered.id);
    TEST_ASSERT_EQUAL_INT32(original.current, recovered.current);
}

TEST_CASE("uplink_roundtrip_i32_extremes", "[uplink]") {
    int32_t values[] = { INT32_MIN, -1, 0, 1, INT32_MAX };
    for (int i = 0; i < 5; i++) {
        uplink_message_t msg = { .id = "rt", .current = values[i] };
        char json[256];
        uplink_to_json(&msg, json, sizeof(json));

        uplink_message_t back;
        sv_error_t err = uplink_from_json(json, &back);
        TEST_ASSERT_EQUAL(SV_OK, err);
        TEST_ASSERT_EQUAL_INT32(values[i], back.current);
    }
}

// ── Null safety ──

TEST_CASE("uplink_serialize_null_msg", "[uplink]") {
    char buf[256];
    TEST_ASSERT_EQUAL(-1, uplink_to_json(NULL, buf, sizeof(buf)));
}

TEST_CASE("uplink_deserialize_null_json", "[uplink]") {
    uplink_message_t msg;
    TEST_ASSERT_EQUAL(SV_ERR_DESERIALIZATION, uplink_from_json(NULL, &msg));
}

TEST_CASE("uplink_deserialize_invalid_json", "[uplink]") {
    uplink_message_t msg;
    TEST_ASSERT_EQUAL(SV_ERR_DESERIALIZATION, uplink_from_json("not json", &msg));
}
