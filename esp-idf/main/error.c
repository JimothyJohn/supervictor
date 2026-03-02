#include "error.h"

const char *sv_error_str(sv_error_t err) {
    switch (err) {
    case SV_OK:                  return "OK";
    case SV_ERR_DESERIALIZATION: return "Failed to deserialize response";
    case SV_ERR_PARSE:           return "Failed to parse response";
    case SV_ERR_WIFI:            return "WiFi error";
    case SV_ERR_TCP:             return "TCP connection error";
    case SV_ERR_TLS:             return "TLS error";
    case SV_ERR_DNS:             return "DNS resolution error";
    case SV_ERR_TIMEOUT:         return "Operation timed out";
    case SV_ERR_BUFFER_OVERFLOW: return "Buffer overflow";
    default:                     return "Unknown error";
    }
}
