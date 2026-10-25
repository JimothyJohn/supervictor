/// Industrial machine profiles the simulator can impersonate.
pub mod profile;
/// Deterministic telemetry simulator (seeded, `no_std`).
pub mod sim;
/// Uplink message and API response types for device-to-cloud communication.
pub mod uplink;

#[cfg(test)]
mod uplink_tests;
