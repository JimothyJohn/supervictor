#include "uplink_task.h"
#include "config.h"
#include "models/uplink.h"
#include "network/http.h"
#include "network/tls.h"
#include "app/wifi_sta.h"

#include "esp_log.h"
#include "esp_tls.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>

static const char *TAG = "uplink";

void uplink_task(void *pvParameters) {
    (void)pvParameters;

    // Wait for WiFi
    wifi_sta_wait_connected(UINT32_MAX);
    ESP_LOGI(TAG, "WiFi connected, starting uplink loop");

    uplink_message_t msg = {
        .id = "1234567890",
        .current = 100,
    };

    esp_tls_cfg_t tls_cfg = tls_create_config();

    while (1) {
        // Serialize uplink message
        char json_body[SV_JSON_BUF];
        int json_len = uplink_to_json(&msg, json_body, sizeof(json_body));
        if (json_len < 0) {
            ESP_LOGE(TAG, "JSON serialization failed");
            vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
            continue;
        }

        // Build HTTP POST request
        char request[SV_HTTP_REQ_BUF];
        int req_len = http_post_request(request, sizeof(request),
                                        SV_HOST, json_body, "/");
        if (req_len < 0) {
            ESP_LOGE(TAG, "Request too large for buffer");
            vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
            continue;
        }

        // Open TLS connection (handles DNS + TCP + TLS handshake)
        esp_tls_t *tls = esp_tls_init();
        if (!tls) {
            ESP_LOGE(TAG, "esp_tls_init failed");
            vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
            continue;
        }

        int ret = esp_tls_conn_new_sync(SV_HOST, strlen(SV_HOST),
                                         SV_AWS_PORT, &tls_cfg, tls);
        if (ret != 1) {
            ESP_LOGE(TAG, "TLS connection failed");
            esp_tls_conn_destroy(tls);
            vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
            continue;
        }

        // Write request
        size_t written = 0;
        size_t to_write = (size_t)req_len;
        while (written < to_write) {
            ret = esp_tls_conn_write(tls, request + written,
                                     to_write - written);
            if (ret >= 0) {
                written += (size_t)ret;
            } else {
                ESP_LOGE(TAG, "Write failed: %d", ret);
                break;
            }
        }

        // Read response
        char response[SV_HTTP_RESP_BUF];
        int total = 0;
        do {
            ret = esp_tls_conn_read(tls, response + total,
                                    sizeof(response) - (size_t)total - 1);
            if (ret > 0) {
                total += ret;
            }
        } while (ret > 0 && total < (int)(sizeof(response) - 1));
        response[total] = '\0';

        // Parse response
        lambda_response_t parsed;
        if (http_parse_response(response, &parsed) == SV_OK) {
            ESP_LOGI(TAG, "Body: %s", parsed.body);
        } else {
            ESP_LOGW(TAG, "Failed to parse response");
        }

        esp_tls_conn_destroy(tls);
        vTaskDelay(pdMS_TO_TICKS(SV_LOOP_DELAY_MS));
    }
}
