#pragma once

#include <stdint.h>
#include <stddef.h>
#include "error.h"

// Matches Rust UplinkMessage { id: HString<64>, current: i32 }
typedef struct {
    char id[65];        // 64 chars + null terminator
    int32_t current;
} uplink_message_t;

// Matches Rust LambdaResponse (parsed from HTTP response headers + body)
typedef struct {
    char x_amzn_request_id[65];   // HString<64>
    char x_amz_apigw_id[33];     // HString<32>
    char x_amzn_trace_id[129];   // HString<128>
    char content_type[33];       // HString<32>
    char content_length[9];      // HString<8>
    char date[33];               // HString<32>
    char body[1025];             // HString<1024>
} lambda_response_t;

// Serialize uplink message to JSON using cJSON.
// Returns bytes written to buf (excluding null), or -1 on error.
int uplink_to_json(const uplink_message_t *msg, char *buf, size_t buf_size);

// Deserialize uplink message from JSON using cJSON.
sv_error_t uplink_from_json(const char *json, uplink_message_t *out);
