#pragma once

// --- Host (from Kconfig) ---
#define SV_HOST              CONFIG_SV_HOST

// --- Network Timing (milliseconds) ---
#define SV_POLL_INTERVAL_MS  2000
#define SV_WIFI_RETRY_MS     5000
#define SV_DNS_RETRY_MS      3000

// --- TCP / Socket ---
#define SV_TCP_RX_BUF        4096
#define SV_TCP_TX_BUF        4096
#define SV_TLS_RX_BUF        4096
#define SV_SOCKET_TIMEOUT_S  10

// --- TLS ---
#define SV_AWS_PORT          443
#define SV_TLS_TIMEOUT_S     15

// --- HTTP ---
#define SV_HTTP_REQ_BUF      512
#define SV_HTTP_RESP_BUF     1024
#define SV_HTTP_READ_TIMEOUT_S 5

// --- Application Logic ---
#define SV_LOOP_DELAY_MS     5000
#define SV_JSON_BUF          128

// --- Portal (AP mode) ---
#define SV_DEVICE_ID         "supervictor"
#define SV_AP_SSID           "Supervictor"
#define SV_AP_GATEWAY        { 192, 168, 4, 1 }
#define SV_PORTAL_PORT       80
#define SV_PORTAL_TX_BUF     4096
#define SV_PORTAL_RX_BUF     512
#define SV_PORTAL_TIMEOUT_S  30

// --- Firmware version ---
#define SV_FW_VERSION        "0.1.0"
