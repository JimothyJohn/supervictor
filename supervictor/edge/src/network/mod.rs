/// DNS hijack server for captive portal mode.
pub mod dns;
/// HTTP request building and response parsing (no_std, zero-alloc).
pub mod http;
/// Captive portal HTTP server serving the configuration UI.
pub mod server;
/// mTLS certificate loading for esp-mbedtls.
#[cfg(feature = "embedded")]
pub mod tls;
