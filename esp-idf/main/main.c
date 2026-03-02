#include "config.h"
#include "app/wifi_sta.h"
#include "app/uplink_task.h"

#include "esp_log.h"
#include "nvs_flash.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "main";

void app_main(void) {
    // Initialize NVS (required by WiFi driver)
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES ||
        ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        nvs_flash_erase();
        ret = nvs_flash_init();
    }
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "NVS init failed: %s", esp_err_to_name(ret));
        return;
    }

    // Start WiFi STA
    ret = wifi_sta_init(CONFIG_SV_WIFI_SSID, CONFIG_SV_WIFI_PASSWORD);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "WiFi init failed: %s", esp_err_to_name(ret));
        return;
    }

    // Spawn uplink task
    xTaskCreate(uplink_task, "uplink", 8192, NULL, 5, NULL);

    ESP_LOGI(TAG, "Supervictor started (ESP-IDF)");
}
