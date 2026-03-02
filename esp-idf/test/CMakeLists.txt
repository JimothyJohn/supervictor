# Unity test component for ESP-IDF
# Run with: idf.py build && idf.py flash monitor (on target)
# Or build for linux host target for CI

idf_component_register(
    SRCS
        "test_error.c"
        "test_uplink.c"
        "test_http.c"
        "test_dns.c"
        "test_server.c"
    INCLUDE_DIRS "../main"
    REQUIRES unity json main
)
