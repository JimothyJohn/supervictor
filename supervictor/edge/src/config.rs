//! Application Configuration Constants

use embassy_time::Duration;

/// Remote API hostname, set at compile time.
pub const HOST: &str = env!("HOST");
// CERT_PATH / CA_PATH have no runtime consts: network/tls.rs consumes them
// directly via env! inside include_str! at compile time.

// --- System ---
/// Total heap allocation in bytes for the ESP32 allocator.
///
/// Sized against the main stack, not just heap demand: the ESP32's static
/// DRAM segment is ~196 KB and the boot stack — which the Embassy executor
/// runs EVERY task's poll on — is whatever the linker has left after .data
/// and .bss (heap included). At 144 KB the leftover stack was 5 KB and the
/// firmware stack-overflowed inside embassy_net::new before WiFi even came
/// up; 112 KB leaves ~37 KB of stack, enough for mbedtls handshakes on
/// Xtensa's stack-hungry windowed ABI. Check `_stack_start - _stack_end` in
/// the ELF before raising this.
pub const HEAP_SIZE: usize = 112 * 1024;

// --- Network Timing ---
/// Interval between network stack readiness checks.
pub const NETWORK_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(2000);
/// Delay before retrying a failed WiFi connection.
pub const WIFI_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(5000);
/// Delay before retrying a failed DNS resolution.
pub const DNS_RETRY_DELAY: Duration = Duration::from_millis(3000);

// --- Ethernet (EMAC / RMII, ESP32-POE-ISO) ---
/// RX frame slots in the EMAC DMA ring. Sized for DHCP/TLS-handshake bursts;
/// with TX below this is ~19 KB of DRAM held statically.
pub const EMAC_RX_FRAMES: usize = 8;
/// TX frame slots in the EMAC DMA ring (telemetry uplink is low-rate).
pub const EMAC_TX_FRAMES: usize = 4;
/// Per-frame DMA buffer size in bytes (max Ethernet frame + slack).
pub const EMAC_FRAME_BUF_SIZE: usize = 1600;
/// Interval between PHY link-status polls.
pub const ETH_LINK_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Wait after enabling PHY power (GPIO12) before MDIO access. The LAN8710A
/// needs sub-ms per datasheet; generous margin costs boot time once.
pub const PHY_POWER_STABLE_DELAY_MS: u32 = 100;

// --- TCP/Socket ---
/// TCP receive buffer size in bytes.
pub const TCP_RX_BUFFER_SIZE: usize = 4096;
/// TLS receive buffer size in bytes.
pub const TLS_RX_BUFFER_SIZE: usize = 4096;
/// TCP transmit buffer size in bytes.
pub const TCP_TX_BUFFER_SIZE: usize = 4096;
/// Timeout for TCP socket operations.
pub const SOCKET_TIMEOUT: Duration = Duration::from_secs(10);

// --- TLS ---
/// TLS port used for the remote API endpoint.
pub const AWS_IOT_PORT: u16 = 443;
// mbedTLS debug level (0=off, 1=Error, 2=StateChanges, 3=Info, 4=Verbose).
/// mbedTLS debug verbosity level for debug builds.
#[cfg(debug_assertions)]
pub const TLS_DEBUG_LEVEL: u32 = 2;
/// mbedTLS debug verbosity level for release builds (off).
#[cfg(not(debug_assertions))]
pub const TLS_DEBUG_LEVEL: u32 = 0;
/// Maximum time allowed for the TLS handshake to complete.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

// --- HTTP ---
/// Timeout for reading an HTTP response from the server.
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Heapless buffer capacity for outgoing HTTP requests.
pub const HTTP_REQUEST_BUFFER_CAPACITY: usize = 512;
/// Heapless buffer capacity for incoming HTTP responses.
pub const HTTP_RESPONSE_BUFFER_CAPACITY: usize = 1024;

// --- Application Logic ---
/// Delay between iterations of the main uplink loop on success.
pub const MAIN_LOOP_DELAY: Duration = Duration::from_millis(5000);
/// Initial backoff delay after a transient failure (DNS, TCP, TLS, HTTP).
pub const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
/// Maximum backoff delay — caps exponential growth.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Maximum byte length of a single JSON map key.
pub const JSON_MAP_KEY_CAPACITY: usize = 8;
/// Maximum byte length of a single JSON map value.
pub const JSON_MAP_VALUE_CAPACITY: usize = 16;
/// Maximum number of entries in a JSON map.
pub const JSON_MAP_ENTRIES_CAPACITY: usize = 2;
/// Maximum byte length of a serialized JSON payload.
pub const JSON_SERIALIZED_CAPACITY: usize = 128;

// --- Portal (AP mode) ---
/// Human-readable device identifier used in portal status responses.
pub const DEVICE_ID: &str = "supervictor";
/// SSID broadcast by the device when in access-point mode.
pub const AP_SSID: &str = "Supervictor";
/// IPv4 gateway address for the captive portal network.
pub const AP_GATEWAY: [u8; 4] = [192, 168, 4, 1];
/// TCP port the captive portal HTTP server listens on.
pub const PORTAL_PORT: u16 = 80;
/// Transmit buffer size for portal HTTP connections.
pub const PORTAL_TX_BUFFER_SIZE: usize = 4096;
/// Receive buffer size for portal HTTP connections.
pub const PORTAL_RX_BUFFER_SIZE: usize = 512;
/// Inactivity timeout for portal HTTP connections.
pub const PORTAL_TIMEOUT: Duration = Duration::from_secs(30);
