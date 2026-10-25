/// DNS hijack server for captive portal mode.
pub mod dns;
/// ESP-NOW beacon + listener beside the wired uplink (coexistence spike).
#[cfg(feature = "espnow")]
pub mod espnow;
/// Wired Ethernet uplink (EMAC + LAN8710A) for the ESP32-POE-ISO.
#[cfg(feature = "ethernet")]
pub mod ethernet;
/// HTTP request building and response parsing (no_std, zero-alloc).
pub mod http;
/// Captive portal HTTP server serving the configuration UI.
pub mod server;
/// mTLS certificate loading for esp-mbedtls.
#[cfg(feature = "embedded")]
pub mod tls;
