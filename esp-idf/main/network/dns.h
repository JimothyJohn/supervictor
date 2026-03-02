#pragma once

#include <stddef.h>
#include <stdint.h>

// Find the end of a DNS QNAME starting at offset.
// Returns the offset of the byte after the terminating 0, or -1 on error.
int dns_find_qname_end(const uint8_t *data, size_t data_len, size_t offset);

// Build a DNS A-record response for any query.
// Copies transaction ID and question, appends a single A record with gateway_ip.
// Returns bytes written to resp_buf, or -1 on error.
int dns_build_response(const uint8_t *query, size_t query_len,
                       uint8_t *resp_buf, size_t resp_buf_size,
                       const uint8_t gateway_ip[4]);
