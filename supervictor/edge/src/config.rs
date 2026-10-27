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
///
/// Primary heap, every classic-ESP32 build: 80 KB here plus the 96 KB
/// second heap in [`DRAM2_HEAP_SIZE`]. 112 KB was the figure before the
/// radio blob and the OTA statics: the ESP-NOW build overflowed an 18 KB
/// stack at 112 KB (2026-10-25), and with OTA's buffers in .bss the WiFi
/// build was down to 26 KB. 80 KB leaves ~40-55 KB of stack on every
/// variant; the heap it gives up comes back threefold from dram2. The
/// two-heap layout soaked on poe-iso-rack-02/03 (espnow) from 19:30Z.
#[cfg(feature = "chip-esp32")]
pub const HEAP_SIZE: usize = 80 * 1024;
/// Second heap region in the ESP32's `dram2_seg` (0x3ffe7e30, 98768 bytes:
/// the DRAM after the ROM data/stack reservations, which esp-hal maps as
/// `.dram2_uninit` and nothing else claims). Plain DMA-capable internal RAM,
/// so the WiFi driver's buffers are fine there.
#[cfg(feature = "chip-esp32")]
pub const DRAM2_HEAP_SIZE: usize = 96 * 1024;
/// Total heap allocation in bytes for the ESP32-C3 allocator.
///
/// The C3's RAM is one 400 KB segment, so the boot stack has ample leftover
/// at this size (hardware-verified pre-Xtensa-retarget at 144 KB).
#[cfg(feature = "chip-esp32c3")]
pub const HEAP_SIZE: usize = 144 * 1024;

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

// --- ESP-NOW (coexistence spike, `espnow` feature) ---
/// WiFi channel the spike boards meet on. There is no AP to follow, so every
/// board pins the same one.
pub const ESPNOW_CHANNEL: u8 = 1;
/// Interval between broadcast beacons.
pub const ESPNOW_BEACON_INTERVAL: Duration = Duration::from_secs(5);
/// Interval between beacon/heap summary lines on the console.
pub const ESPNOW_REPORT_INTERVAL: Duration = Duration::from_secs(60);

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
/// Heapless buffer capacity for outgoing HTTP requests. Mirrored as a
/// literal in `network/http.rs` (this module is embedded-gated; http.rs is
/// host-testable) — keep the two in sync: 768 bytes of JSON body plus
/// ~150 of headers.
pub const HTTP_REQUEST_BUFFER_CAPACITY: usize = 1024;
/// Heapless buffer capacity for incoming HTTP responses.
pub const HTTP_RESPONSE_BUFFER_CAPACITY: usize = 1024;

// --- Application Logic ---
/// Parse a compile-time env var as u32, falling back to `default` when the
/// var is unset. Non-numeric or overflowing values fail the build.
const fn parse_env_u32(v: Option<&str>, default: u32) -> u32 {
    match v {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            assert!(!b.is_empty(), "expected a positive integer");
            let mut i = 0;
            let mut n: u32 = 0;
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "expected a positive integer");
                n = n * 10 + (b[i] - b'0') as u32;
                i += 1;
            }
            n
        }
    }
}

/// Seconds between uplinks (t), settable per build via the UPLINK_INTERVAL_S
/// env var (like DEVICE_NAME). This is the canonical knob — a user-facing
/// "frequency" option is just the inverse (f = 1/t) at the UI layer. The
/// value is reported in every uplink (`interval_s`) so the cloud can judge
/// staleness against the device's own cadence.
pub const UPLINK_INTERVAL_S: u32 = parse_env_u32(option_env!("UPLINK_INTERVAL_S"), 5);
const _: () = assert!(UPLINK_INTERVAL_S > 0, "UPLINK_INTERVAL_S must be >= 1");

/// Delay between iterations of the main uplink loop on success.
pub const MAIN_LOOP_DELAY: Duration = Duration::from_secs(UPLINK_INTERVAL_S as u64);
/// Samples per uplink interval, shipped as an 8-bit `levels` burst (one
/// byte per register per sample; `models::uplink::LEVELS_SAMPLES_MAX`
/// caps it). 50 over the default 5 s interval = a sample every 100 ms
/// (10 Hz). Decided 2026-10-25: the clamp module's peak detector rises
/// within a mains cycle, so 10 Hz resolves inrush and a motor start; its
/// decay is a 10 s RC either way (HARDWARE.md 6.1). Faster than this the
/// sensor adds nothing and every buffer below doubles again.
pub const UPLINK_BURST: usize = 50;
/// Initial backoff delay after a transient failure (DNS, TCP, TLS, HTTP).
pub const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
/// Maximum backoff delay — caps exponential growth.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// RTC watchdog window: the board resets unless a successful uplink
/// round-trip feeds the RWDT within this window. Must comfortably exceed
/// boot→first-uplink (~2 min worst observed on WiFi) and a full BACKOFF_MAX
/// retry cycle, or healthy devices reboot-loop. Plain u64 seconds — the
/// esp-hal Duration is constructed at the arming site in embedded_main.
pub const WATCHDOG_TIMEOUT_S: u64 = 300;
const _: () = assert!(WATCHDOG_TIMEOUT_S > 2 * BACKOFF_MAX.as_secs());
/// Maximum byte length of a single JSON map key.
pub const JSON_MAP_KEY_CAPACITY: usize = 8;
/// Maximum byte length of a single JSON map value.
pub const JSON_MAP_VALUE_CAPACITY: usize = 16;
/// Maximum number of entries in a JSON map.
pub const JSON_MAP_ENTRIES_CAPACITY: usize = 2;
/// Maximum byte length of a serialized JSON payload. Mirrored as a literal
/// in `network/http.rs::post_request` (see HTTP_REQUEST_BUFFER_CAPACITY
/// note); sized for a worst-case full-telemetry UplinkMessage.
pub const JSON_SERIALIZED_CAPACITY: usize = 512;
