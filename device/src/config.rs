//! Application Configuration Constants

use embassy_time::Duration;

pub const HOST: &str = env!("HOST");
pub const CA_PATH: &str = env!("CA_PATH");
pub const CERT_PATH: &str = env!("CERT_PATH");
pub const KEY_PATH: &str = env!("KEY_PATH");

// --- System ---
pub const HEAP_SIZE: usize = 144 * 1024;

// --- Network Timing ---
pub const NETWORK_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(2000);
pub const WIFI_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(5000);
pub const DNS_RETRY_DELAY: Duration = Duration::from_millis(3000);

// --- TCP/Socket ---
pub const TCP_RX_BUFFER_SIZE: usize = 4096;
pub const TLS_RX_BUFFER_SIZE: usize = 4096;
pub const TCP_TX_BUFFER_SIZE: usize = 4096;
pub const SOCKET_TIMEOUT: Duration = Duration::from_secs(10);

// --- TLS ---
pub const AWS_IOT_PORT: u16 = 443;
// mbedTLS debug level (0=off, 1=Error, 2=StateChanges, 3=Info, 4=Verbose).
#[cfg(debug_assertions)]
pub const TLS_DEBUG_LEVEL: u32 = 2;
#[cfg(not(debug_assertions))]
pub const TLS_DEBUG_LEVEL: u32 = 0;
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

// --- HTTP ---
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(5);
pub const HTTP_REQUEST_BUFFER_CAPACITY: usize = 512;
pub const HTTP_RESPONSE_BUFFER_CAPACITY: usize = 1024;

// --- Application Logic ---
pub const MAIN_LOOP_DELAY: Duration = Duration::from_millis(5000);
pub const JSON_MAP_KEY_CAPACITY: usize = 8;
pub const JSON_MAP_VALUE_CAPACITY: usize = 16;
pub const JSON_MAP_ENTRIES_CAPACITY: usize = 2;
pub const JSON_SERIALIZED_CAPACITY: usize = 128;

// --- Portal (AP mode) ---
pub const DEVICE_ID: &str = "supervictor";
pub const AP_SSID: &str = "Supervictor";
pub const AP_GATEWAY: [u8; 4] = [192, 168, 4, 1];
pub const PORTAL_PORT: u16 = 80;
pub const PORTAL_TX_BUFFER_SIZE: usize = 4096;
pub const PORTAL_RX_BUFFER_SIZE: usize = 512;
pub const PORTAL_TIMEOUT: Duration = Duration::from_secs(30);
