#pragma once

#include <stdint.h>

// Matches Rust NfcConfig
typedef struct {
    char device_id[33];         // HString<32>
    uint16_t max_payload_bytes;
    uint16_t timeout_ms;
    uint8_t retry_count;
} nfc_config_t;

// Matches Rust NfcRecord
typedef struct {
    char uid[17];               // HString<16>
    char record_type[17];       // HString<16>
    char payload[129];          // HString<128>
} nfc_record_t;

// Default config with given device_id. Truncates if id > 32 chars.
static inline nfc_config_t nfc_config_default(const char *device_id) {
    nfc_config_t cfg = {
        .device_id = {0},
        .max_payload_bytes = 128,
        .timeout_ms = 1000,
        .retry_count = 3,
    };
    if (device_id) {
        size_t len = 0;
        while (device_id[len] && len < 32) len++;
        // Only copy if it fits (matches Rust behavior: empty on overflow)
        if (device_id[len] == '\0') {
            for (size_t i = 0; i < len; i++) cfg.device_id[i] = device_id[i];
        }
    }
    return cfg;
}
