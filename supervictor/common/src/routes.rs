/// Root endpoint — hello GET + uplink POST
pub const ROOT: &str = "/";

/// Health check
pub const HEALTH: &str = "/health";

/// Device collection — GET (list), POST (register)
pub const DEVICES: &str = "/devices";

/// Single device — GET. Axum pattern: /devices/{device_id}
pub const DEVICE_PATTERN: &str = "/devices/{device_id}";

/// Device uplinks — GET. Axum pattern: /devices/{device_id}/uplinks
pub const DEVICE_UPLINKS_PATTERN: &str = "/devices/{device_id}/uplinks";

/// Per-device power threshold — PUT (admin mTLS). Axum pattern:
/// /devices/{device_id}/threshold
pub const DEVICE_THRESHOLD_PATTERN: &str = "/devices/{device_id}/threshold";

/// Per-device chart style (colour, line, marker) — PUT (admin mTLS). Axum
/// pattern: /devices/{device_id}/style
pub const DEVICE_STYLE_PATTERN: &str = "/devices/{device_id}/style";

/// Fleet status — GET (admin mTLS). Machine-readable per-device health.
pub const FLEET: &str = "/fleet";

/// Fleet summary — GET (admin mTLS). Aggregate counts for alerting.
pub const FLEET_SUMMARY: &str = "/fleet/summary";

/// Fleet history — GET (admin mTLS). Time-bucketed telemetry + status
/// timeline for a `?window=` preset (see `endpoint::history::Window`).
pub const FLEET_HISTORY: &str = "/fleet/history";
