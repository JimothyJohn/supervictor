#include "uplink.h"
#include <string.h>
#include <stdio.h>
#include "cJSON.h"

int uplink_to_json(const uplink_message_t *msg, char *buf, size_t buf_size) {
    if (!msg || !buf || buf_size == 0) {
        return -1;
    }

    cJSON *root = cJSON_CreateObject();
    if (!root) {
        return -1;
    }

    cJSON_AddStringToObject(root, "id", msg->id);
    cJSON_AddNumberToObject(root, "current", msg->current);

    char *json = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);

    if (!json) {
        return -1;
    }

    int len = snprintf(buf, buf_size, "%s", json);
    cJSON_free(json);

    if (len < 0 || (size_t)len >= buf_size) {
        return -1;
    }

    return len;
}

sv_error_t uplink_from_json(const char *json, uplink_message_t *out) {
    if (!json || !out) {
        return SV_ERR_DESERIALIZATION;
    }

    cJSON *root = cJSON_Parse(json);
    if (!root) {
        return SV_ERR_DESERIALIZATION;
    }

    cJSON *id = cJSON_GetObjectItemCaseSensitive(root, "id");
    cJSON *current = cJSON_GetObjectItemCaseSensitive(root, "current");

    if (!cJSON_IsString(id) || !cJSON_IsNumber(current)) {
        cJSON_Delete(root);
        return SV_ERR_DESERIALIZATION;
    }

    // Check id capacity (64 chars max)
    size_t id_len = strlen(id->valuestring);
    if (id_len > 64) {
        cJSON_Delete(root);
        return SV_ERR_DESERIALIZATION;
    }

    memset(out, 0, sizeof(*out));
    strncpy(out->id, id->valuestring, 64);
    out->id[64] = '\0';
    out->current = (int32_t)current->valuedouble;

    cJSON_Delete(root);
    return SV_OK;
}
