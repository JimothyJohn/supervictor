//! `qs` CLI for supervictor — orchestrates dev/staging/prod pipelines,
//! device management, certificate generation, and firmware flashing.

/// Subcommands (dev, edge, staging, prod, certs, ping, truststore, onboard).
pub mod commands;
/// Project-wide path and setting configuration.
pub mod config;
/// `.env` file parsing and environment variable merging.
pub mod env;
/// Central error type for the CLI.
pub mod error;
/// Colored terminal output helpers (milestone, step, success, error, info).
pub mod output;
/// Preflight checks for required CLI tools and Docker.
pub mod preflight;
/// Subprocess execution abstraction with dry-run, logging, and capture support.
pub mod runner;
/// Rust toolchain helpers (e.g. host target detection).
pub mod rust_tools;
/// SAM CLI lifecycle: build, start-api, deploy, and process management.
pub mod sam;
