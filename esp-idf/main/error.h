#pragma once

typedef enum {
    SV_OK = 0,
    SV_ERR_DESERIALIZATION,
    SV_ERR_PARSE,
    SV_ERR_WIFI,
    SV_ERR_TCP,
    SV_ERR_TLS,
    SV_ERR_DNS,
    SV_ERR_TIMEOUT,
    SV_ERR_BUFFER_OVERFLOW,
} sv_error_t;

const char *sv_error_str(sv_error_t err);
