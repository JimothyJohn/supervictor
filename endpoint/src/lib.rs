//! Supervictor endpoint — an axum-based REST API for the supervictor IoT platform.
//!
//! Receives mTLS-authenticated uplinks from ESP32 devices and stores them in
//! SQLite (local/dev) or DynamoDB (production). Deployed via SAM + Lambda Web Adapter.

/// Application configuration loaded from environment variables.
pub mod config;
/// Unified error type with HTTP status mapping.
pub mod error;
/// Framework-agnostic request handlers (pure functions).
pub mod handlers;
/// mTLS client certificate extraction middleware.
pub mod middleware;
/// Domain models, wire types, and API response structs.
pub mod models;
/// Axum router and route handler wiring.
pub mod routes;
/// Pluggable storage backends (SQLite, DynamoDB).
pub mod store;
