#include "http.h"
#include <string.h>
#include <stdio.h>
#include <ctype.h>

int http_get_request(char *buf, size_t buf_size,
                     const char *host, const char *path) {
    if (!path) path = "/";

    int n = snprintf(buf, buf_size,
        "GET %s HTTP/1.0\r\n"
        "Host: %s\r\n"
        "User-Agent: Uplink/0.1.0 (Platform; ESP32-C3)\r\n"
        "Accept: */*\r\n"
        "\r\n",
        path, host);

    if (n < 0 || (size_t)n >= buf_size) {
        return -1;
    }
    return n;
}

int http_post_request(char *buf, size_t buf_size,
                      const char *host, const char *json_body,
                      const char *path) {
    if (!path) path = "/";
    if (!json_body) json_body = "";

    size_t body_len = strlen(json_body);

    int n = snprintf(buf, buf_size,
        "POST %s HTTP/1.1\r\n"
        "Host: %s\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: %zu\r\n"
        "Connection: close\r\n"
        "\r\n"
        "%s",
        path, host, body_len, json_body);

    if (n < 0 || (size_t)n >= buf_size) {
        return -1;
    }
    return n;
}

// Case-insensitive string comparison (ASCII only)
static int strncasecmp_local(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        int ca = tolower((unsigned char)a[i]);
        int cb = tolower((unsigned char)b[i]);
        if (ca != cb) return ca - cb;
        if (ca == '\0') return 0;
    }
    return 0;
}

// Copy src into dst, up to dst_size-1 chars, null-terminate.
// Returns 0 on success, SV_ERR_DESERIALIZATION if src is too long.
static sv_error_t copy_field(char *dst, size_t dst_size, const char *src) {
    size_t len = strlen(src);
    if (len >= dst_size) {
        return SV_ERR_DESERIALIZATION;
    }
    memcpy(dst, src, len);
    dst[len] = '\0';
    return SV_OK;
}

// Trim leading and trailing whitespace in-place, return pointer to trimmed start.
static const char *trim(const char *s, char *out, size_t out_size) {
    while (*s == ' ' || *s == '\t') s++;
    size_t len = strlen(s);
    while (len > 0 && (s[len - 1] == ' ' || s[len - 1] == '\t')) len--;
    if (len >= out_size) len = out_size - 1;
    memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

sv_error_t http_parse_response(const char *response, lambda_response_t *out) {
    if (!response || !out) {
        return SV_ERR_DESERIALIZATION;
    }

    memset(out, 0, sizeof(*out));

    // Must have at least one line (status line)
    if (*response == '\0') {
        return SV_ERR_DESERIALIZATION;
    }

    // Work on a mutable copy
    size_t resp_len = strlen(response);
    // Stack-allocate for typical responses, bail on huge ones
    if (resp_len > 8192) {
        return SV_ERR_BUFFER_OVERFLOW;
    }
    char copy[8193];
    memcpy(copy, response, resp_len);
    copy[resp_len] = '\0';

    // Split into lines (handles both \r\n and \n)
    char *saveptr = NULL;
    char *line = strtok_r(copy, "\n", &saveptr);

    // Skip status line
    if (!line) {
        return SV_ERR_DESERIALIZATION;
    }

    // Parse headers
    int in_body = 0;
    size_t body_pos = 0;

    while ((line = strtok_r(NULL, "\n", &saveptr)) != NULL) {
        // Strip trailing \r
        size_t llen = strlen(line);
        if (llen > 0 && line[llen - 1] == '\r') {
            line[llen - 1] = '\0';
            llen--;
        }

        if (!in_body) {
            // Empty line = end of headers
            if (llen == 0) {
                in_body = 1;
                continue;
            }

            // Find colon separator
            char *colon = strchr(line, ':');
            if (!colon) {
                continue; // Malformed header, skip
            }

            *colon = '\0';
            const char *key = line;
            const char *raw_value = colon + 1;

            // Trim the value
            char trimmed[256];
            trim(raw_value, trimmed, sizeof(trimmed));

            sv_error_t err = SV_OK;

            if (strncasecmp_local(key, "x-amzn-requestid", 16) == 0 && strlen(key) == 16) {
                err = copy_field(out->x_amzn_request_id, sizeof(out->x_amzn_request_id), trimmed);
            } else if (strncasecmp_local(key, "x-amz-apigw-id", 14) == 0 && strlen(key) == 14) {
                err = copy_field(out->x_amz_apigw_id, sizeof(out->x_amz_apigw_id), trimmed);
            } else if (strncasecmp_local(key, "x-amzn-trace-id", 15) == 0 && strlen(key) == 15) {
                err = copy_field(out->x_amzn_trace_id, sizeof(out->x_amzn_trace_id), trimmed);
            } else if (strncasecmp_local(key, "content-type", 12) == 0 && strlen(key) == 12) {
                err = copy_field(out->content_type, sizeof(out->content_type), trimmed);
            } else if (strncasecmp_local(key, "content-length", 14) == 0 && strlen(key) == 14) {
                err = copy_field(out->content_length, sizeof(out->content_length), trimmed);
            } else if (strncasecmp_local(key, "date", 4) == 0 && strlen(key) == 4) {
                err = copy_field(out->date, sizeof(out->date), trimmed);
            }

            if (err != SV_OK) {
                return err;
            }
        } else {
            // Body line
            // Strip trailing \r
            llen = strlen(line);
            if (llen > 0 && line[llen - 1] == '\r') {
                line[llen - 1] = '\0';
                llen--;
            }

            // Add newline separator between body lines
            if (body_pos > 0) {
                if (body_pos >= sizeof(out->body) - 1) {
                    return SV_ERR_DESERIALIZATION;
                }
                out->body[body_pos++] = '\n';
            }

            // Append body line
            if (body_pos + llen >= sizeof(out->body)) {
                return SV_ERR_DESERIALIZATION;
            }
            memcpy(out->body + body_pos, line, llen);
            body_pos += llen;
            out->body[body_pos] = '\0';
        }
    }

    return SV_OK;
}
