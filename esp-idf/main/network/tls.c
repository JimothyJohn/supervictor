#include "tls.h"
#include "config.h"

esp_tls_cfg_t tls_create_config(void) {
    esp_tls_cfg_t cfg = {
        .cacert_buf       = ca_pem_start,
        .cacert_bytes     = (unsigned int)(ca_pem_end - ca_pem_start),
        .clientcert_buf   = client_crt_start,
        .clientcert_bytes = (unsigned int)(client_crt_end - client_crt_start),
        .clientkey_buf    = client_key_start,
        .clientkey_bytes  = (unsigned int)(client_key_end - client_key_start),
        .timeout_ms       = SV_TLS_TIMEOUT_S * 1000,
        .non_block        = false,
    };
    return cfg;
}
