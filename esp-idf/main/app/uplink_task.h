#pragma once

// FreeRTOS task that sends periodic mTLS uplink messages.
// Blocks until WiFi is connected, then loops: POST → parse → sleep.
void uplink_task(void *pvParameters);
