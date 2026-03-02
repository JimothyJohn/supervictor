#include "server.h"
#include <string.h>
#include <stdio.h>
#include "cJSON.h"

int server_parse_request_line(const char *request,
                              char *method, size_t method_size,
                              char *path, size_t path_size) {
    method[0] = '\0';
    path[0] = '\0';

    if (!request || !*request) {
        snprintf(path, path_size, "/");
        return 0;
    }

    // Find end of first line
    const char *eol = strstr(request, "\r\n");
    if (!eol) eol = request + strlen(request);

    size_t line_len = (size_t)(eol - request);
    char line[256];
    if (line_len >= sizeof(line)) line_len = sizeof(line) - 1;
    memcpy(line, request, line_len);
    line[line_len] = '\0';

    // Split by whitespace: METHOD PATH VERSION
    char *saveptr = NULL;
    char *m = strtok_r(line, " \t", &saveptr);
    char *p = strtok_r(NULL, " \t", &saveptr);

    if (m) {
        snprintf(method, method_size, "%s", m);
    }
    if (p) {
        snprintf(path, path_size, "%s", p);
    } else {
        snprintf(path, path_size, "/");
    }

    return 0;
}

const char *server_extract_body(const char *request) {
    if (!request) return "";
    const char *sep = strstr(request, "\r\n\r\n");
    if (!sep) return "";
    return sep + 4;
}

int server_build_status_json(char *buf, size_t buf_size,
                             const char *device_id,
                             const char *fw_version,
                             const char *ip,
                             const char *state) {
    int n = snprintf(buf, buf_size,
        "{\"device_id\":\"%s\","
        "\"fw_version\":\"%s\","
        "\"ip\":\"%s\","
        "\"state\":\"%s\"}",
        device_id, fw_version, ip, state);

    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}

int server_parse_configure_body(const char *body,
                                char *ssid, size_t ssid_size,
                                char *password, size_t password_size) {
    if (!body || !*body) return -1;

    cJSON *root = cJSON_Parse(body);
    if (!root) return -1;

    cJSON *j_ssid = cJSON_GetObjectItemCaseSensitive(root, "ssid");
    cJSON *j_pass = cJSON_GetObjectItemCaseSensitive(root, "password");

    if (!cJSON_IsString(j_ssid) || !cJSON_IsString(j_pass)) {
        cJSON_Delete(root);
        return -1;
    }

    snprintf(ssid, ssid_size, "%s", j_ssid->valuestring);
    snprintf(password, password_size, "%s", j_pass->valuestring);

    cJSON_Delete(root);
    return 0;
}

int server_build_configure_response(char *buf, size_t buf_size,
                                    int ok, const char *message) {
    int n = snprintf(buf, buf_size,
        "{\"ok\":%s,\"message\":\"%s\"}",
        ok ? "true" : "false", message);

    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}

int server_build_response_header(char *buf, size_t buf_size,
                                 const char *content_type,
                                 size_t content_length,
                                 const char *extra_headers) {
    int n;
    if (extra_headers) {
        n = snprintf(buf, buf_size,
            "HTTP/1.0 200 OK\r\n"
            "Content-Type: %s\r\n"
            "Content-Length: %zu\r\n"
            "Connection: close\r\n"
            "%s"
            "\r\n",
            content_type, content_length, extra_headers);
    } else {
        n = snprintf(buf, buf_size,
            "HTTP/1.0 200 OK\r\n"
            "Content-Type: %s\r\n"
            "Content-Length: %zu\r\n"
            "Connection: close\r\n"
            "\r\n",
            content_type, content_length);
    }

    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}

int server_build_redirect(char *buf, size_t buf_size, const char *location) {
    int n = snprintf(buf, buf_size,
        "HTTP/1.0 302 Found\r\n"
        "Location: %s\r\n"
        "Content-Length: 0\r\n"
        "Connection: close\r\n"
        "\r\n",
        location);

    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}

int server_build_error_response(char *buf, size_t buf_size,
                                int status, const char *message) {
    // Body: {"error":"<message>"}
    char body[256];
    int body_len = snprintf(body, sizeof(body), "{\"error\":\"%s\"}", message);
    if (body_len < 0) return -1;

    int n = snprintf(buf, buf_size,
        "HTTP/1.0 %d Error\r\n"
        "Content-Type: application/json\r\n"
        "Connection: close\r\n"
        "Content-Length: %d\r\n"
        "\r\n"
        "%s",
        status, body_len, body);

    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}

int server_format_ip(char *buf, size_t buf_size, const uint8_t octets[4]) {
    int n = snprintf(buf, buf_size, "%u.%u.%u.%u",
                     octets[0], octets[1], octets[2], octets[3]);
    if (n < 0 || (size_t)n >= buf_size) return -1;
    return n;
}
