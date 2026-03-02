#pragma once

#include "esp_err.h"

// Initialize WiFi in AP mode for captive portal.
esp_err_t wifi_ap_init(const char *ssid);
