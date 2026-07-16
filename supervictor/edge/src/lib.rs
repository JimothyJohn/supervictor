//! Supervictor device firmware library for ESP32-family chips (ESP32 /
//! ESP32-C3), primarily the Olimex ESP32-POE-ISO.
//!
//! A `no_std` embedded firmware crate built on the Embassy async runtime,
//! providing mTLS-secured uplink communication, a captive portal for WiFi
//! provisioning, and DNS hijacking for AP mode.

#![no_std]

#[cfg(all(
    feature = "embedded",
    not(any(feature = "chip-esp32", feature = "chip-esp32c3"))
))]
compile_error!(
    "embedded builds need a chip: --features chip-esp32 (target xtensa-esp32-none-elf) or chip-esp32c3 (target riscv32imc-unknown-none-elf); `ethernet` implies chip-esp32"
);
#[cfg(all(feature = "chip-esp32", feature = "chip-esp32c3"))]
compile_error!("chip-esp32 and chip-esp32c3 are mutually exclusive — pick one");

/// Board support (Olimex ESP32-POE-ISO pin map, APLL math, RMII bring-up).
pub mod board;
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
