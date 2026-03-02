#pragma once

#include "esp_err.h"
#include <stdbool.h>
#include <stdint.h>

// Initialize WiFi in STA mode and start connection.
// Registers event handlers for auto-reconnect on disconnect.
esp_err_t wifi_sta_init(const char *ssid, const char *password);

// Block until connected and IP assigned, or timeout.
// Pass portMAX_DELAY for infinite wait.
esp_err_t wifi_sta_wait_connected(uint32_t timeout_ms);

// Check if currently connected with an IP address.
bool wifi_sta_is_connected(void);
