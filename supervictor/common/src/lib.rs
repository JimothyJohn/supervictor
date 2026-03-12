//! Shared `no_std` constants and types for the supervictor workspace.
//!
//! This crate defines the canonical wire types, route paths, field names,
//! and status values used by both the embedded device firmware and the
//! cloud endpoint. Enable the `alloc` feature for serde model structs.

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

/// JSON field-name constants shared across device and endpoint.
pub mod fields;
/// Serde-serializable request/response models (requires `alloc`).
#[cfg(feature = "alloc")]
pub mod models;
/// HTTP route path constants.
pub mod routes;
/// Device status value constants.
pub mod status;
