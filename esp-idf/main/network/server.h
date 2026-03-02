#pragma once

#include <stddef.h>
#include <stdint.h>

// Extract HTTP method and path from the first request line.
// method and path are written into caller-provided buffers.
// Returns 0 on success.
int server_parse_request_line(const char *request,
                              char *method, size_t method_size,
                              char *path, size_t path_size);

// Extract the body after the \r\n\r\n header separator.
// Returns pointer into the original request string, or "" if no body.
const char *server_extract_body(const char *request);

// Build the JSON body for GET /api/status.
// Returns bytes written (excluding null), or -1 on overflow.
int server_build_status_json(char *buf, size_t buf_size,
                             const char *device_id,
                             const char *fw_version,
                             const char *ip,
                             const char *state);

// Parse WiFi config from POST /api/configure body.
// Returns 0 on success, -1 on invalid JSON.
int server_parse_configure_body(const char *body,
                                char *ssid, size_t ssid_size,
                                char *password, size_t password_size);

// Build a JSON configure response.
// Returns bytes written (excluding null), or -1 on overflow.
int server_build_configure_response(char *buf, size_t buf_size,
                                    int ok, const char *message);

// Build an HTTP 200 response header.
// extra_headers may be NULL. Returns bytes written, or -1 on overflow.
int server_build_response_header(char *buf, size_t buf_size,
                                 const char *content_type,
                                 size_t content_length,
                                 const char *extra_headers);

// Build an HTTP 302 redirect response.
// Returns bytes written, or -1 on overflow.
int server_build_redirect(char *buf, size_t buf_size,
                          const char *location);

// Build an HTTP error response with JSON body.
// Returns bytes written, or -1 on overflow.
int server_build_error_response(char *buf, size_t buf_size,
                                int status, const char *message);

// Format IPv4 octets as dotted-decimal string.
// Returns bytes written (excluding null), or -1 on overflow.
int server_format_ip(char *buf, size_t buf_size, const uint8_t octets[4]);
