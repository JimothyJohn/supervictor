/// Industrial machine profiles the simulator can impersonate (feature
/// `sim`; always available to tests).
#[cfg(any(test, feature = "sim"))]
pub mod profile;
/// Seeded xorshift RNG and the per-board seed (always compiled).
pub mod rng;
/// Deterministic telemetry simulator (seeded, `no_std`). Behind `sim`
/// (off by default, decided 2026-10-27) so a production image cannot
/// report a reading it did not take; tests always compile it.
#[cfg(any(test, feature = "sim"))]
pub mod sim;
/// Uplink message and API response types for device-to-cloud communication.
pub mod uplink;

#[cfg(test)]
mod uplink_tests;
