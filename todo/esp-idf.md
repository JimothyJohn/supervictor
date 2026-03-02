# Port device/ to ESP-IDF (C)

## Why

The firmware runs nightly Rust on `riscv32imc-unknown-none-elf` with Embassy async, `no_std`, and a deep dependency tree (`esp-hal` beta, `esp-mbedtls` pinned to a git rev, `esp-wifi`, `embassy-*`). Every upstream breaking change requires coordinated updates across ~15 crates on an unstable toolchain. ESP-IDF is Espressif's first-party C SDK — stable releases, LTS branches, mature docs, broad community support, and a single version to pin.

The firmware is simple: connect WiFi, POST JSON over mTLS, parse response, sleep, repeat. The complexity lives in the toolchain, not the application logic.

## Current Rust Architecture

```
device/src/
├── lib.rs                  # #![no_std] crate root
├── config.rs               # Constants: HOST, buffer sizes, timeouts
├── error.rs                # HttpError { Deserialization, GenericParseError }
├── models/
│   ├── uplink.rs           # UplinkMessage { id: HString<64>, current: i32 }
│   ├── uplink_tests.rs     # 17 tests
│   ├── nfc.rs              # NfcConfig, NfcRecord (future)
│   └── nfc_tests.rs        # 17 tests
├── network/
│   ├── http.rs             # get_request(), post_request(), parse_response()
│   ├── http_edge_tests.rs  # 42 tests
│   ├── http_gap_tests.rs   # 16 tests
│   ├── dns.rs              # find_qname_end(), build_dns_response()
│   ├── dns_tests.rs        # 16 tests
│   ├── server.rs           # Portal HTTP server (AP captive portal)
│   ├── server_tests.rs     # 25 tests
│   └── tls.rs              # load_certificates() via include_str!()
├── app/
│   └── tasks.rs            # Embassy async: connection, net_task, app
└── bin/
    ├── embedded_main.rs    # ESP32-C3 entry (#[esp_hal_embassy::main])
    ├── desktop_main.rs     # Desktop mTLS test client (tokio + rustls)
    └── portal_main.rs      # Portal AP mode
```

### Application flow

1. Init ESP32-C3 peripherals (clock, timers, RNG)
2. Init WiFi STA, spawn reconnection task
3. Wait for DHCP IP
4. Main loop (every 5s):
   - TCP connect to `HOST:443`
   - Load compiled-in certs (CA + client cert + client key)
   - TLS handshake (mTLS, TLS 1.3, 15s timeout)
   - POST `{"id":"1234567890","current":100}` to `/`
   - Parse response headers + body → `LambdaResponse`
   - Log, sleep 5s

### Key dependencies replaced

| Rust Crate | Purpose | ESP-IDF Replacement |
|---|---|---|
| `esp-hal` (beta) | Hardware abstraction | ESP-IDF HAL (built-in) |
| `esp-wifi` | WiFi driver | `esp_wifi` component |
| `esp-mbedtls` (git rev) | TLS | `esp_tls` (wraps mbedtls) |
| `embassy-executor` | Async runtime | FreeRTOS tasks |
| `embassy-net` | TCP/DNS/DHCP | lwIP (built-in) |
| `embassy-time` | Timers | `vTaskDelay()` |
| `serde` + `serde-json-core` | JSON | cJSON (bundled) |
| `heapless` | Fixed-size containers | Fixed-size `char[]` / `uint8_t[]` |
| `static_cell` | Static allocation | Global variables |

## Target Directory Structure

New project at `device-idf/`, sibling to `device/`. Both coexist during transition.

```
device-idf/
├── CMakeLists.txt
├── sdkconfig.defaults
├── partitions.csv
├── CLAUDE.md
├── main/
│   ├── CMakeLists.txt
│   ├── Kconfig.projbuild
│   ├── main.c
│   ├── config.h
│   ├── error.h
│   ├── error.c
│   ├── models/
│   │   ├── uplink.h
│   │   ├── uplink.c
│   │   └── nfc.h
│   ├── network/
│   │   ├── http.h
│   │   ├── http.c
│   │   ├── tls.h
│   │   ├── tls.c
│   │   ├── dns.h
│   │   ├── dns.c
│   │   ├── server.h
│   │   └── server.c
│   └── app/
│       ├── wifi_sta.h
│       ├── wifi_sta.c
│       ├── wifi_ap.h
│       ├── wifi_ap.c
│       └── uplink_task.c
├── test/
│   ├── CMakeLists.txt
│   ├── test_error.c
│   ├── test_uplink.c
│   ├── test_http.c
│   ├── test_dns.c
│   └── test_server.c
├── host_test/
│   ├── CMakeLists.txt
│   └── main/
│       ├── CMakeLists.txt
│       └── host_test_main.c
└── docs/
    └── index.html
```

## File-by-File Mapping

| Rust Source | C Target | Notes |
|---|---|---|
| `lib.rs` | `main.c` | `app_main()` replaces `#[esp_hal_embassy::main]` |
| `config.rs` | `config.h` | `#define` constants + `Kconfig.projbuild` for runtime config |
| `error.rs` | `error.h` + `error.c` | `typedef enum sv_error_t` |
| `models/uplink.rs` | `models/uplink.h` + `uplink.c` | cJSON replaces serde |
| `models/nfc.rs` | `models/nfc.h` | Structs only, no impl yet |
| `network/http.rs` | `network/http.h` + `http.c` | `snprintf()` replaces heapless push chains |
| `network/tls.rs` | `network/tls.h` + `tls.c` | `EMBED_TXTFILES` replaces `include_str!()` |
| `network/dns.rs` | `network/dns.h` + `dns.c` | Nearly 1:1, pure C functions |
| `network/server.rs` | `network/server.h` + `server.c` | `esp_http_server` replaces raw TCP |
| `app/tasks.rs` | `app/wifi_sta.c` + `uplink_task.c` | FreeRTOS tasks replace Embassy tasks |
| `bin/embedded_main.rs` | `main.c` | Peripheral init via ESP-IDF APIs |
| `bin/desktop_main.rs` | `host_test/` | Linux target build |

## Phased Implementation

### Phase 0 — Project scaffold

Bootable ESP-IDF project that prints "Hello" over serial.

**Files**: `CMakeLists.txt`, `main/CMakeLists.txt`, `main/main.c`, `sdkconfig.defaults`, `partitions.csv`

**Top-level CMakeLists.txt**:
```cmake
cmake_minimum_required(VERSION 3.16)
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(supervictor)
```

**main/CMakeLists.txt**:
```cmake
idf_component_register(
    SRCS
        "main.c"
        "error.c"
        "models/uplink.c"
        "network/http.c"
        "network/tls.c"
        "network/dns.c"
        "network/server.c"
        "app/wifi_sta.c"
        "app/wifi_ap.c"
        "app/uplink_task.c"
    INCLUDE_DIRS "."
    EMBED_TXTFILES
        "${PROJECT_DIR}/../certs/AmazonRootCA1.pem"
        "${PROJECT_DIR}/../cloud/certs/devices/test-device/client.pem"
        "${PROJECT_DIR}/../cloud/certs/devices/test-device/client.key"
)
```

**Kconfig.projbuild** (replaces Rust `env!()` macros):
```
menu "Supervictor"
    config SV_HOST
        string "Target host"
        default "supervictor.advin.io"
    config SV_WIFI_SSID
        string "WiFi SSID"
    config SV_WIFI_PASSWORD
        string "WiFi Password"
endmenu
```

**sdkconfig.defaults**:
```
CONFIG_PARTITION_TABLE_CUSTOM=y
CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="partitions.csv"
CONFIG_ESP_TLS_USING_MBEDTLS=y
CONFIG_MBEDTLS_KEY_EXCHANGE_RSA=y
CONFIG_MBEDTLS_KEY_EXCHANGE_ECDHE_RSA=y
CONFIG_MBEDTLS_KEY_EXCHANGE_ECDHE_ECDSA=y
CONFIG_MBEDTLS_SSL_PROTO_TLS1_3=y
CONFIG_ESPTOOLPY_FLASHSIZE_4MB=y
CONFIG_FREERTOS_HZ=1000
CONFIG_ESP_MAIN_TASK_STACK_SIZE=8192
```

**Verify**: `idf.py set-target esp32c3 && idf.py build && idf.py flash monitor`

---

### Phase 1 — Error types and models

Port data structures with cJSON serialization.

**config.h**:
```c
#pragma once

#define SV_HOST              CONFIG_SV_HOST
#define SV_TCP_RX_BUF        4096
#define SV_TCP_TX_BUF        4096
#define SV_TLS_RX_BUF        4096
#define SV_HTTP_REQ_BUF      512
#define SV_HTTP_RESP_BUF     1024
#define SV_JSON_BUF          128
#define SV_WIFI_RETRY_MS     5000
#define SV_POLL_INTERVAL_MS  2000
#define SV_SOCKET_TIMEOUT_S  10
#define SV_TLS_TIMEOUT_S     15
#define SV_HTTP_TIMEOUT_S    5
#define SV_LOOP_DELAY_MS     5000
#define SV_AWS_PORT          443
#define SV_AP_SSID           "Supervictor"
#define SV_AP_GATEWAY        {192, 168, 4, 1}
#define SV_PORTAL_PORT       80
```

**error.h**:
```c
#pragma once

typedef enum {
    SV_OK = 0,
    SV_ERR_DESERIALIZATION,
    SV_ERR_PARSE,
    SV_ERR_WIFI,
    SV_ERR_TCP,
    SV_ERR_TLS,
    SV_ERR_DNS,
    SV_ERR_TIMEOUT,
    SV_ERR_BUFFER_OVERFLOW,
} sv_error_t;

const char *sv_error_str(sv_error_t err);
```

**models/uplink.h**:
```c
#pragma once
#include <stdint.h>
#include <stddef.h>
#include "error.h"

typedef struct {
    char id[65];        // 64 chars + null (matches HString<64>)
    int32_t current;
} uplink_message_t;

typedef struct {
    char x_amzn_request_id[65];
    char x_amz_apigw_id[33];
    char x_amzn_trace_id[129];
    char content_type[33];
    char content_length[9];
    char date[33];
    char body[1025];
} lambda_response_t;

int uplink_to_json(const uplink_message_t *msg, char *buf, size_t buf_size);
sv_error_t uplink_from_json(const char *json, uplink_message_t *out);
```

**models/uplink.c** uses cJSON (bundled with ESP-IDF):
```c
#include "cJSON.h"

int uplink_to_json(const uplink_message_t *msg, char *buf, size_t buf_size) {
    cJSON *root = cJSON_CreateObject();
    cJSON_AddStringToObject(root, "id", msg->id);
    cJSON_AddNumberToObject(root, "current", msg->current);
    char *json = cJSON_PrintUnformatted(root);
    int len = snprintf(buf, buf_size, "%s", json);
    cJSON_free(json);
    cJSON_Delete(root);
    return len < (int)buf_size ? len : -1;
}
```

**Verify**: Port all 17 uplink tests and 17 NFC tests to Unity. Run on host.

---

### Phase 2 — HTTP request builders and response parser

Port `network/http.rs` — the hand-rolled HTTP client functions.

**network/http.h**:
```c
#pragma once
#include <stddef.h>
#include "error.h"
#include "models/uplink.h"

int http_get_request(char *buf, size_t buf_size,
                     const char *host, const char *path);

int http_post_request(char *buf, size_t buf_size,
                      const char *host, const char *json_body,
                      const char *path);

sv_error_t http_parse_response(const char *response,
                               lambda_response_t *out);
```

GET builder produces:
```
GET <path> HTTP/1.0\r\nHost: <host>\r\nUser-Agent: Uplink/0.1.0 (Platform; ESP32-C3)\r\nAccept: */*\r\n\r\n
```

POST builder produces:
```
POST <path> HTTP/1.1\r\nHost: <host>\r\nContent-Type: application/json\r\nContent-Length: <N>\r\n\r\n<json>
```

Response parser splits on `\r\n`, matches headers case-insensitively (`strncasecmp`), extracts body after `\r\n\r\n`. Key headers: `x-amzn-RequestId`, `x-amz-apigw-id`, `X-Amzn-Trace-Id`, `content-type`, `content-length`, `date`.

**Critical**: every `snprintf` return value checked, every `strncpy` null-terminated. The Rust version has compile-time capacity limits via `HString<N>` — in C, bounds must be checked manually at every copy.

**Verify**: Port all 61 HTTP tests (3 inline + 42 edge + 16 gap). The adversarial tests from `http_gap_tests.rs` are especially important for catching C buffer issues.

---

### Phase 3 — DNS hijack and portal server

Port pure functions from `dns.rs` and `server.rs`.

**network/dns.h**:
```c
#pragma once
#include <stddef.h>
#include <stdint.h>

int dns_find_qname_end(const uint8_t *data, size_t data_len, size_t offset);

int dns_build_response(const uint8_t *query, size_t query_len,
                       uint8_t *resp_buf, size_t resp_buf_size,
                       const uint8_t gateway_ip[4]);
```

Logic from `dns.rs` maps nearly 1:1. The `HVec<u8, 512>` becomes a caller-provided buffer.

**network/server.h**:
```c
#pragma once
#include <stddef.h>

int server_parse_request_line(const char *request,
                              char *method, size_t method_size,
                              char *path, size_t path_size);

const char *server_extract_body(const char *request);

int server_build_status_json(char *buf, size_t buf_size,
                             const char *device_id,
                             const char *ip, const char *state);

int server_parse_configure_body(const char *body,
                                char *ssid, size_t ssid_size,
                                char *password, size_t password_size);

int server_build_response_header(char *buf, size_t buf_size,
                                 const char *content_type,
                                 size_t content_length);

int server_build_redirect(char *buf, size_t buf_size,
                          const char *location);

int server_build_error_response(char *buf, size_t buf_size,
                                int status, const char *message);

int server_format_ip(char *buf, size_t buf_size,
                     const uint8_t octets[4]);
```

**Verify**: Port all 16 DNS tests (byte-exact packet fixtures) and 25 server tests.

---

### Phase 4 — WiFi STA

Event-driven WiFi connection with auto-reconnect, replacing Embassy async.

**app/wifi_sta.h**:
```c
#pragma once
#include "esp_err.h"

esp_err_t wifi_sta_init(const char *ssid, const char *password);
esp_err_t wifi_sta_wait_connected(uint32_t timeout_ms);
bool wifi_sta_is_connected(void);
```

Implementation uses ESP-IDF event loop:
- `WIFI_EVENT_STA_DISCONNECTED` → `vTaskDelay(5s)` then `esp_wifi_connect()`
- `IP_EVENT_STA_GOT_IP` → `xEventGroupSetBits(WIFI_CONNECTED_BIT)`

The Rust `connection()` task (lines 17-46 of `tasks.rs`) becomes an event handler callback — simpler, no task needed.

**Verify**: Flash to hardware, confirm WiFi connects and reconnects after AP reboot.

---

### Phase 5 — mTLS uplink task

The core application loop. DNS resolve, TLS handshake, POST, parse response.

**network/tls.h**:
```c
#pragma once
#include "esp_tls.h"

// Cert symbols generated by EMBED_TXTFILES
extern const uint8_t ca_pem_start[]     asm("_binary_AmazonRootCA1_pem_start");
extern const uint8_t ca_pem_end[]       asm("_binary_AmazonRootCA1_pem_end");
extern const uint8_t client_pem_start[] asm("_binary_client_pem_start");
extern const uint8_t client_pem_end[]   asm("_binary_client_pem_end");
extern const uint8_t client_key_start[] asm("_binary_client_key_start");
extern const uint8_t client_key_end[]   asm("_binary_client_key_end");

esp_tls_cfg_t tls_create_config(void);
```

**tls.c**:
```c
esp_tls_cfg_t tls_create_config(void) {
    return (esp_tls_cfg_t){
        .cacert_buf       = ca_pem_start,
        .cacert_bytes     = ca_pem_end - ca_pem_start,
        .clientcert_buf   = client_pem_start,
        .clientcert_bytes = client_pem_end - client_pem_start,
        .clientkey_buf    = client_key_start,
        .clientkey_bytes  = client_key_end - client_key_start,
        .timeout_ms       = SV_TLS_TIMEOUT_S * 1000,
        .non_block        = false,
    };
}
```

**app/uplink_task.c**:
```c
void uplink_task(void *pvParameters) {
    wifi_sta_wait_connected(portMAX_DELAY);

    uplink_message_t msg = { .id = "1234567890", .current = 100 };
    esp_tls_cfg_t tls_cfg = tls_create_config();

    while (1) {
        char json_body[SV_JSON_BUF];
        uplink_to_json(&msg, json_body, sizeof(json_body));

        char request[SV_HTTP_REQ_BUF];
        http_post_request(request, sizeof(request), SV_HOST, json_body, "/");

        esp_tls_t *tls = esp_tls_init();
        if (esp_tls_conn_new_sync(SV_HOST, strlen(SV_HOST),
                                   SV_AWS_PORT, &tls_cfg, tls) == 1) {
            // Write request
            size_t written = 0;
            while (written < strlen(request)) {
                int ret = esp_tls_conn_write(tls, request + written,
                                             strlen(request) - written);
                if (ret >= 0) written += ret;
                else break;
            }

            // Read response
            char response[SV_HTTP_RESP_BUF];
            int total = 0;
            int ret;
            do {
                ret = esp_tls_conn_read(tls, response + total,
                                        sizeof(response) - total - 1);
                if (ret > 0) total += ret;
            } while (ret > 0);
            response[total] = '\0';

            // Parse
            lambda_response_t parsed;
            if (http_parse_response(response, &parsed) == SV_OK) {
                ESP_LOGI(TAG, "Body: %s", parsed.body);
            }
        } else {
            ESP_LOGE(TAG, "TLS connection failed");
        }

        esp_tls_conn_destroy(tls);
        vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
    }
}
```

**main.c**:
```c
void app_main(void) {
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES ||
        ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        nvs_flash_erase();
        nvs_flash_init();
    }

    wifi_sta_init(CONFIG_SV_WIFI_SSID, CONFIG_SV_WIFI_PASSWORD);

    xTaskCreate(uplink_task, "uplink", 8192, NULL, 5, NULL);
}
```

**Verify**: Flash to hardware, confirm mTLS POST to `supervictor.advin.io` succeeds. Compare response with Rust firmware output.

---

### Phase 6 — Portal / AP mode

Captive portal with DNS hijack and HTTP config server.

**app/wifi_ap.c**: `esp_wifi_set_mode(WIFI_MODE_AP)` + `esp_netif_create_default_wifi_ap()`

**DNS hijack task**: UDP socket on port 53, calls `dns_build_response()` for every query, responds with gateway IP.

```c
void dns_hijack_task(void *pvParameters) {
    int sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    struct sockaddr_in addr = {
        .sin_family = AF_INET,
        .sin_port = htons(53),
        .sin_addr.s_addr = INADDR_ANY
    };
    bind(sock, (struct sockaddr *)&addr, sizeof(addr));

    uint8_t query[512], resp[512];
    while (1) {
        struct sockaddr_in client;
        socklen_t len = sizeof(client);
        int n = recvfrom(sock, query, sizeof(query), 0,
                         (struct sockaddr *)&client, &len);
        if (n > 0) {
            uint8_t gw[] = SV_AP_GATEWAY;
            int resp_len = dns_build_response(query, n, resp, sizeof(resp), gw);
            if (resp_len > 0) {
                sendto(sock, resp, resp_len, 0,
                       (struct sockaddr *)&client, len);
            }
        }
    }
}
```

**Portal HTTP server**: Use `esp_http_server` with URI handlers for `/`, `/api/status`, `/api/configure`, and wildcard redirect.

**Verify**: Flash, connect phone to "Supervictor" AP, confirm captive portal redirects and serves config page.

---

### Phase 7 — NVS storage (new feature)

Store WiFi credentials from portal config in NVS. Not present in the Rust version (marked as TODO).

```c
esp_err_t nvs_store_wifi_config(const char *ssid, const char *password);
esp_err_t nvs_load_wifi_config(char *ssid, size_t ssid_size,
                                char *password, size_t password_size);
bool nvs_has_wifi_config(void);
```

On boot: check NVS for stored credentials → use those if present, else start portal AP mode.

---

### Phase 8 — Testing

**~148 tests to port**, all using Unity (bundled with ESP-IDF).

| Rust Test File | Count | C Test File | Target |
|---|---|---|---|
| `uplink_tests.rs` | 17 | `test_uplink.c` | Host |
| `nfc_tests.rs` | 17 | `test_nfc.c` | Host |
| `http.rs` (inline) | 3 | `test_http.c` | Host |
| `http_edge_tests.rs` | 42 | `test_http.c` | Host |
| `http_gap_tests.rs` | 16 | `test_http.c` | Host |
| `dns_tests.rs` | 16 | `test_dns.c` | Host |
| `server_tests.rs` | 25 | `test_server.c` | Host |
| `error_tests.rs` | 5 | `test_error.c` | Host |
| `mock_tcp_roundtrip.rs` | 7 | Integration | Host |

All pure-function tests run on the **ESP-IDF Linux host target** (`idf.py --preview set-target linux`). No hardware needed for unit tests.

TCP loopback integration tests use pthreads on the Linux target to spawn mock servers, same pattern as the Rust `one_shot_server()` + `send_and_receive()`.

Compiler flags: `-Wall -Wextra -Werror -Wformat-security`. AddressSanitizer on host tests.

---

### Phase 9 — CI

`.github/workflows/esp_idf_ci.yml`:

```yaml
name: ESP-IDF CI
on:
  push:
    paths: ['device-idf/**']
  pull_request:
    paths: ['device-idf/**']

jobs:
  build:
    runs-on: ubuntu-latest
    container:
      image: espressif/idf:v5.4
    steps:
      - uses: actions/checkout@<pinned-sha>
      - name: Build firmware
        run: |
          cd device-idf
          idf.py set-target esp32c3
          idf.py build

  host-tests:
    runs-on: ubuntu-latest
    container:
      image: espressif/idf:v5.4
    steps:
      - uses: actions/checkout@<pinned-sha>
      - name: Run host tests
        run: |
          cd device-idf/host_test
          idf.py --preview set-target linux
          idf.py build
          ./build/host_test.elf
```

---

### Phase 10 — Quickstart integration

Add `--idf` flag to `qs edge`. New file `quickstart/commands/edge_idf.py`:

```python
def run_edge_idf(args, config):
    require(["idf.py"])
    env_vars = load_env(config.env_dev)

    # Write Kconfig overrides
    idf_dir = config.repo_root / "device-idf"
    runner.run(["idf.py", "build", "flash", "monitor"],
               cwd=idf_dir, env=make_env(env_vars))
```

Update `edge.py` to dispatch: `qs edge` → Rust (default), `qs edge --idf` → ESP-IDF.

---

## Migration Path

1. Both `device/` and `device-idf/` coexist — same certs, same cloud API
2. `qs edge` defaults to Rust, `qs edge --idf` uses ESP-IDF
3. Validate parity: both POST identical JSON, get identical responses
4. Once ESP-IDF passes all tests + 24-hour soak on hardware, archive `device/`
5. Rename `device-idf/` → `device/`

## Risks and Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| **Buffer overflows** — no compile-time capacity enforcement like `HString<N>` | High | Sized buffers everywhere, `snprintf` with return checks, `-Werror`, ASan on host tests |
| **TLS memory** — mbedtls may need different heap tuning | High | Measure with `heap_caps_get_free_size()`, tune `MBEDTLS_SSL_IN_CONTENT_LEN` / `MBEDTLS_SSL_OUT_CONTENT_LEN` in menuconfig |
| **cJSON malloc** — dynamic allocation unlike zero-alloc serde-json-core | Low | Allocations are small (~100 bytes) and short-lived, freed immediately with `cJSON_Delete()`. Response parsing uses the hand-rolled parser (no cJSON). |
| **TLS 1.3** — Rust version uses TLS 1.3, cloud requires TLS 1.2 minimum | Medium | Enable `CONFIG_MBEDTLS_SSL_PROTO_TLS1_3=y`. Test both TLS 1.2 and 1.3. Falls back gracefully. |
| **Embassy→FreeRTOS** — cooperative async vs preemptive threads | Medium | Firmware is simple (2-3 tasks, no shared mutable state). FreeRTOS is actually simpler for this use case. Use event groups for synchronization. |
| **ESP-IDF version drift** | Low | Pin to ESP-IDF v5.4 LTS in CI container and `CLAUDE.md`. |
