//! End-to-end mTLS integration test against the deployed supervictor API.
//!
//! Uses tokio + rustls with client certificates to connect to the production
//! or staging endpoint — the same TLS path as the desktop binary.
//!
//! Requires:
//!   - cargo feature "desktop" (gates tokio + rustls dependencies)
//!   - API_ENDPOINT env var (e.g. "https://supervictor.advin.io")
//!   - TEST_CERT_DIR env var pointing to certs/ directory containing:
//!       - AmazonRootCA1.pem (CA chain)
//!       - devices/test-device/client.pem (client cert)
//!       - devices/test-device/client.key (private key)
//!
//! Run:
//!   API_ENDPOINT=https://supervictor.advin.io \
//!   TEST_CERT_DIR=../cloud/certs \
//!     cargo test --test remote_mtls_roundtrip --features desktop \
//!       --target aarch64-apple-darwin
//!
//! This test is intentionally a skeleton — full implementation deferred until
//! mock TCP and SAM local tests are proven in CI.

#![cfg(feature = "desktop")]

#[test]
fn remote_get_hello_with_mtls() {
    let endpoint = match std::env::var("API_ENDPOINT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP: API_ENDPOINT not set");
            return;
        }
    };
    let cert_dir = match std::env::var("TEST_CERT_DIR") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP: TEST_CERT_DIR not set");
            return;
        }
    };

    eprintln!("TODO: implement mTLS GET to {endpoint} using certs from {cert_dir}");
    // Implementation outline:
    // 1. Load CA cert from {cert_dir}/AmazonRootCA1.pem
    // 2. Load client cert from {cert_dir}/devices/test-device/client.pem
    // 3. Load client key from {cert_dir}/devices/test-device/client.key
    // 4. Build rustls::ClientConfig with root store + client auth
    // 5. DNS resolve + TCP connect
    // 6. TLS handshake via tokio_rustls::TlsConnector
    // 7. Send get_request() bytes
    // 8. Read response, parse with parse_response()
    // 9. Assert body contains "Hello from Supervictor!"
}

#[test]
fn remote_post_uplink_with_mtls() {
    let endpoint = match std::env::var("API_ENDPOINT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP: API_ENDPOINT not set");
            return;
        }
    };
    let cert_dir = match std::env::var("TEST_CERT_DIR") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP: TEST_CERT_DIR not set");
            return;
        }
    };

    eprintln!("TODO: implement mTLS POST to {endpoint} using certs from {cert_dir}");
    // Implementation outline:
    // 1-6. Same TLS setup as above
    // 7. Build UplinkMessage { id: "rust-mtls-test", current: 999 }
    // 8. Send post_request() bytes
    // 9. Read response, parse with parse_response()
    // 10. Assert body contains "Uplink received" and "rust-mtls-test"
}
