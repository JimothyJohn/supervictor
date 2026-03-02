# supervictor esp-idf

C-based ESP-IDF firmware for ESP32-C3 with mTLS uplink to AWS Lambda.

## Commands
```
idf.py set-target esp32c3
idf.py build
idf.py flash monitor
```

## Configuration
WiFi and host are set via `idf.py menuconfig` → Supervictor menu, or in `sdkconfig.defaults`.

## Architecture
```
main/
  main.c              # app_main() entry point
  config.h            # Constants (timeouts, buffer sizes)
  error.h / error.c   # sv_error_t enum
  models/uplink.h/c   # uplink_message_t, lambda_response_t, cJSON serialization
  models/nfc.h        # nfc_config_t, nfc_record_t (future)
  network/http.h/c    # GET/POST builders, response parser
  network/tls.h/c     # Certificate loading (embedded via EMBED_TXTFILES)
  network/dns.h/c     # DNS hijack pure functions (portal mode)
  network/server.h/c  # Portal HTTP server pure functions
  app/wifi_sta.h/c    # WiFi STA event-driven connection
  app/wifi_ap.h/c     # WiFi AP mode for portal
  app/uplink_task.h/c # Main uplink loop (FreeRTOS task)
```

## Key Patterns
- **Scheduler**: FreeRTOS tasks, not Embassy async
- **TLS**: esp_tls (wraps mbedtls)
- **JSON**: cJSON (bundled with ESP-IDF)
- **Certs**: embedded via CMake EMBED_TXTFILES
- **Strings**: fixed-size char arrays with bounds-checked snprintf
- **Tests**: Unity framework, runnable on Linux host target
