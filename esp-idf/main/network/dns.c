#include "dns.h"
#include <string.h>

int dns_find_qname_end(const uint8_t *data, size_t data_len, size_t offset) {
    while (1) {
        if (offset >= data_len) {
            return -1;
        }
        uint8_t label_len = data[offset];
        if (label_len == 0) {
            return (int)(offset + 1);
        }
        // Pointer (compression)
        if (label_len >= 0xC0) {
            return (int)(offset + 2);
        }
        offset += 1 + label_len;
    }
}

int dns_build_response(const uint8_t *query, size_t query_len,
                       uint8_t *resp_buf, size_t resp_buf_size,
                       const uint8_t gateway_ip[4]) {
    // Minimum DNS query: 12-byte header + 1-byte name + 4-byte QTYPE/QCLASS
    if (query_len < 17) {
        return -1;
    }

    int qname_end = dns_find_qname_end(query, query_len, 12);
    if (qname_end < 0) {
        return -1;
    }

    size_t question_end = (size_t)qname_end + 4; // +2 QTYPE + 2 QCLASS
    if (question_end > query_len) {
        return -1;
    }

    size_t question_len = question_end - 12;

    // Response = 12 header + question + 16 answer (2 name ptr + 2 type + 2 class + 4 TTL + 2 rdlen + 4 IP)
    size_t resp_len = 12 + question_len + 16;
    if (resp_len > resp_buf_size) {
        return -1;
    }

    size_t pos = 0;

    // Transaction ID (bytes 0-1)
    resp_buf[pos++] = query[0];
    resp_buf[pos++] = query[1];

    // Flags: standard response, recursion available
    resp_buf[pos++] = 0x81;
    resp_buf[pos++] = 0x80;

    // Question count (from query)
    resp_buf[pos++] = query[4];
    resp_buf[pos++] = query[5];

    // Answer count: 1
    resp_buf[pos++] = 0x00;
    resp_buf[pos++] = 0x01;

    // Authority RR count: 0
    resp_buf[pos++] = 0x00;
    resp_buf[pos++] = 0x00;

    // Additional RR count: 0
    resp_buf[pos++] = 0x00;
    resp_buf[pos++] = 0x00;

    // Question section (copied from query)
    memcpy(resp_buf + pos, query + 12, question_len);
    pos += question_len;

    // Answer section: A record
    resp_buf[pos++] = 0xC0; // Name pointer to offset 12
    resp_buf[pos++] = 0x0C;
    resp_buf[pos++] = 0x00; // Type: A
    resp_buf[pos++] = 0x01;
    resp_buf[pos++] = 0x00; // Class: IN
    resp_buf[pos++] = 0x01;
    resp_buf[pos++] = 0x00; // TTL: 60 seconds
    resp_buf[pos++] = 0x00;
    resp_buf[pos++] = 0x00;
    resp_buf[pos++] = 0x3C;
    resp_buf[pos++] = 0x00; // Data length: 4
    resp_buf[pos++] = 0x04;

    // Gateway IP
    resp_buf[pos++] = gateway_ip[0];
    resp_buf[pos++] = gateway_ip[1];
    resp_buf[pos++] = gateway_ip[2];
    resp_buf[pos++] = gateway_ip[3];

    return (int)pos;
}
