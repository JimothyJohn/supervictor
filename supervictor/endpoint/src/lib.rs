//! Supervictor endpoint — an axum-based REST API for the supervictor IoT platform.
//!
//! Receives mTLS-authenticated uplinks from ESP32 devices and stores them in
//! SQLite (local/dev) or DynamoDB (production). Deployed via SAM + Lambda Web Adapter.

/// Application configuration loaded from environment variables.
pub mod config;
/// Unified error type with HTTP status mapping.
pub mod error;
/// Fleet health: staleness classification and machine-readable snapshots.
pub mod fleet;
/// Framework-agnostic request handlers (pure functions).
pub mod handlers;
/// Fleet history: bucketed telemetry and status timeline (pure functions).
pub mod history;
/// Live view data: uplinks as timestamped 8-bit register samples.
pub mod live;
/// mTLS client certificate extraction middleware.
pub mod middleware;
/// Domain models, wire types, and API response structs.
pub mod models;
/// Axum router and route handler wiring.
pub mod routes;
/// Pluggable storage backends (SQLite, DynamoDB).
pub mod store;
/// Minimal UTC RFC 3339 timestamp formatting (chrono replacement).
pub mod time;
/// In-binary mTLS termination: TLS listener + verified peer identity (feature `tls`).
#[cfg(feature = "tls")]
pub mod tls;
/// Fleet dashboard: server-rendered HTML + SSE behind admin mTLS (feature `ui`).
#[cfg(feature = "ui")]
pub mod ui;
/// Request payload validation beyond what serde enforces.
pub mod validate;
/// Periodic fleet-health logging for long-running deployments.
pub mod watchdog;
