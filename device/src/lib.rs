//! Supervictor device firmware library for ESP32-C3.
//!
//! A `no_std` embedded firmware crate built on the Embassy async runtime,
//! providing mTLS-secured uplink communication, a captive portal for WiFi
//! provisioning, and DNS hijacking for AP mode.

#![no_std]

/// HTTP error types for request/response handling.
pub mod error;
/// Domain models for device-to-cloud communication.
pub mod models;
/// Networking primitives: HTTP, DNS, TLS, and portal server.
pub mod network;

/// Embassy async application tasks (WiFi, networking, main loop).
#[cfg(feature = "embedded")]
pub mod app;
/// Compile-time configuration constants (timings, buffer sizes, endpoints).
#[cfg(feature = "embedded")]
pub mod config;
