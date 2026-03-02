idf_component_register(
    SRCS
        "main.c"
        "error.c"
        "models/uplink.c"
        "network/http.c"
        "network/dns.c"
        "network/server.c"
        "network/tls.c"
        "app/wifi_sta.c"
        "app/wifi_ap.c"
        "app/uplink_task.c"
    INCLUDE_DIRS "."
    EMBED_TXTFILES
        "${CMAKE_SOURCE_DIR}/../certs/AmazonRootCA1.pem"
        "${CMAKE_SOURCE_DIR}/../certs/devices/supervictor/client.crt"
        "${CMAKE_SOURCE_DIR}/../certs/devices/supervictor/client.key"
)
