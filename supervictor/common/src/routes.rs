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

/// Per-device target firmware — PUT (admin mTLS), body
/// `{"desired_fw": "0.2.0"}` or `null` to clear. Axum pattern:
/// /devices/{device_id}/desired_fw
pub const DEVICE_DESIRED_FW_PATTERN: &str = "/devices/{device_id}/desired_fw";

/// One device's firmware image for one version — PUT (admin mTLS: upload
/// the raw image), GET (that device's own certificate or admin: download,
/// `Range` honoured), DELETE (admin). Axum pattern:
/// /devices/{device_id}/firmware/{version}
pub const DEVICE_FIRMWARE_PATTERN: &str = "/devices/{device_id}/firmware/{version}";

/// The image's manifest — GET (that device's own certificate or admin).
/// Axum pattern: /devices/{device_id}/firmware/{version}/manifest
pub const DEVICE_FIRMWARE_MANIFEST_PATTERN: &str =
    "/devices/{device_id}/firmware/{version}/manifest";

/// Bench-only fault injection on a device's next image download — PUT
/// (admin mTLS) with `{"stall_at": <byte offset>, "stall_secs": <s>}` arms
/// a one-shot stall, `null`/`{}` clears it; GET (admin) reads the armed
/// fault or 404. In-memory: a restart forgets it. Axum pattern:
/// /devices/{device_id}/firmware_fault
pub const DEVICE_FIRMWARE_FAULT_PATTERN: &str = "/devices/{device_id}/firmware_fault";

/// Fleet status — GET (admin mTLS). Machine-readable per-device health.
pub const FLEET: &str = "/fleet";

/// Fleet summary — GET (admin mTLS). Aggregate counts for alerting.
pub const FLEET_SUMMARY: &str = "/fleet/summary";

/// Fleet history — GET (admin mTLS). Time-bucketed telemetry + status
/// timeline for a `?window=` preset (see `endpoint::history::Window`).
pub const FLEET_HISTORY: &str = "/fleet/history";
