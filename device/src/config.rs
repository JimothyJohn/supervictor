//! Application Configuration Constants

// AI-Generated comment: Import Duration for time-based constants.
use embassy_time::Duration;

pub const HOST: &str = env!("HOST");
pub const CA_PATH: &str = "../../certs/AmazonRootCA1.pem";
pub const CERT_PATH: &str = "../../certs/temp-250423.crt";
pub const KEY_PATH: &str = "../../certs/temp-250423.key";

// --- System ---
// AI-Generated comment: Size of the heap allocator. Tuning might be needed based on memory usage.
pub const HEAP_SIZE: usize = 144 * 1024;

// --- Network Timing ---
// AI-Generated comment: Interval for checking WiFi link status and acquired IP address.
pub const NETWORK_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(2000);
// AI-Generated comment: Delay before retrying WiFi connection after failure or disconnect.
pub const WIFI_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(5000);
// AI-Generated comment: Delay before retrying DNS resolution after failure.
pub const DNS_RETRY_DELAY: Duration = Duration::from_millis(3000);

// --- TCP/Socket ---
// AI-Generated comment: Size of the TCP receive buffer. Should accommodate largest expected TLS record/HTTP response.
pub const TCP_RX_BUFFER_SIZE: usize = 4096;
pub const TLS_RX_BUFFER_SIZE: usize = 4096;
// AI-Generated comment: Size of the TCP transmit buffer. Should accommodate largest expected TLS record/HTTP request.
pub const TCP_TX_BUFFER_SIZE: usize = 4096;
// AI-Generated comment: Timeout for TCP socket operations like connect.
pub const SOCKET_TIMEOUT: Duration = Duration::from_secs(10);

// --- TLS ---
// AI-Generated comment: Port for AWS IoT HTTPS endpoint with mTLS authentication.
pub const AWS_IOT_PORT: u16 = 443;
// AI-Generated comment: mbedTLS debug level (0=off, 1=Error, 2=StateChanges, 3=Info, 4=Verbose).
// TODO: Set to 0 for release builds.
pub const TLS_DEBUG_LEVEL: u32 = 4;
// AI-Generated comment: Timeout for the TLS handshake (session.connect) process.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

// --- HTTP ---
// AI-Generated comment: Timeout for reading the HTTP response body.
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(5);
// AI-Generated comment: Max size of the formatted HTTP request string.
pub const HTTP_REQUEST_BUFFER_CAPACITY: usize = 512;
// AI-Generated comment: Buffer size for reading HTTP response content.
pub const HTTP_RESPONSE_BUFFER_CAPACITY: usize = 1024;

// --- Application Logic ---
// AI-Generated comment: Delay between sending POST requests in the main application loop.
pub const MAIN_LOOP_DELAY: Duration = Duration::from_millis(5000);
// AI-Generated comment: Capacities for the heapless map used for the JSON payload.
pub const JSON_MAP_KEY_CAPACITY: usize = 8;
pub const JSON_MAP_VALUE_CAPACITY: usize = 16;
pub const JSON_MAP_ENTRIES_CAPACITY: usize = 2;
// AI-Generated comment: Max size of the serialized JSON string payload.
pub const JSON_SERIALIZED_CAPACITY: usize = 128;

// --- Portal (AP mode) ---
pub const DEVICE_ID: &str = "supervictor";
pub const AP_SSID: &str = "Supervictor";
pub const AP_GATEWAY: [u8; 4] = [192, 168, 4, 1];
pub const PORTAL_PORT: u16 = 80;
pub const PORTAL_TX_BUFFER_SIZE: usize = 4096;
pub const PORTAL_RX_BUFFER_SIZE: usize = 512;
pub const PORTAL_TIMEOUT: Duration = Duration::from_secs(30);
