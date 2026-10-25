//! Board support. Each board module exposes the same surface (pin constants,
//! `ethernet_mac`, `enable_rmii_clk_out`); `active` is the one this firmware
//! is built for, so the network layer never names a board.

/// Olimex ESP32-POE2: POE-ISO's pin map with the RMII clock on GPIO0.
pub mod poe2;
/// Olimex ESP32-POE-ISO: pin map, APLL clock math, and RMII bring-up.
pub mod poe_iso;

#[cfg(feature = "board-poe2")]
pub use poe2 as active;
/// The board this build targets. `ethernet` alone is the POE-ISO; the
/// `board-poe2` feature selects the POE2.
#[cfg(not(feature = "board-poe2"))]
pub use poe_iso as active;

#[cfg(test)]
mod poe2_tests;
#[cfg(test)]
mod poe_iso_tests;
