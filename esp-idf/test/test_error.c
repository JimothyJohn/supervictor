#include "unity.h"
#include "error.h"
#include <string.h>

TEST_CASE("error_str_ok", "[error]") {
    TEST_ASSERT_EQUAL_STRING("OK", sv_error_str(SV_OK));
}

TEST_CASE("error_str_deserialization", "[error]") {
    const char *msg = sv_error_str(SV_ERR_DESERIALIZATION);
    TEST_ASSERT_NOT_NULL(strstr(msg, "deserialize"));
}

TEST_CASE("error_str_parse", "[error]") {
    const char *msg = sv_error_str(SV_ERR_PARSE);
    TEST_ASSERT_NOT_NULL(strstr(msg, "parse"));
}

TEST_CASE("error_str_variants_distinct", "[error]") {
    TEST_ASSERT_NOT_EQUAL(0,
        strcmp(sv_error_str(SV_ERR_DESERIALIZATION),
              sv_error_str(SV_ERR_PARSE)));
}

TEST_CASE("error_str_all_non_null", "[error]") {
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_OK));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_DESERIALIZATION));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_PARSE));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_WIFI));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_TCP));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_TLS));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_DNS));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_TIMEOUT));
    TEST_ASSERT_NOT_NULL(sv_error_str(SV_ERR_BUFFER_OVERFLOW));
}
