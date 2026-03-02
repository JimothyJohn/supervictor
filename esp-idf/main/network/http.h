#pragma once

#include <stddef.h>
#include "error.h"
#include "models/uplink.h"

// Build an HTTP GET request string.
// Returns bytes written (excluding null), or -1 on overflow.
int http_get_request(char *buf, size_t buf_size,
                     const char *host, const char *path);

// Build an HTTP POST request with a pre-serialized JSON body.
// Returns bytes written (excluding null), or -1 on overflow.
int http_post_request(char *buf, size_t buf_size,
                      const char *host, const char *json_body,
                      const char *path);

// Parse an HTTP response into a lambda_response_t.
// Extracts known headers (case-insensitive) and body.
sv_error_t http_parse_response(const char *response, lambda_response_t *out);
