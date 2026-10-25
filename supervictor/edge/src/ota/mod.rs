//! Over-the-air update (docs/OTA.md).
//!
//! Pull, never push: the uplink response carries `desired_fw`; when it is
//! newer than what runs, the device fetches that version's manifest and
//! image over the same mTLS channel, streams the image into the inactive
//! A/B slot, verifies size + SHA-256 (and the ESP-IDF header) *before*
//! switching the boot slot, then reboots. The new image counts as working
//! only once it has completed an uplink round-trip; until then every boot
//! is counted in RTC memory and the third unconfirmed boot flips the slot
//! back. The bootloader espflash ships selects slots but does not roll
//! back on its own, so this crate does.
//!
//! Everything but [`device`] is `no_std` + host-testable with no hardware
//! in the loop: the installer is driven through [`install::Slot`], the
//! HTTP framing is bytes in / facts out, and the boot policy is a pure
//! function over an eight-word record. The stress tests live beside them.

/// Flash + bootloader glue and the update task (embedded builds only).
#[cfg(feature = "embedded")]
pub mod device;
/// Request builders and response-head parsing for the two OTA GETs.
pub mod http;
/// Sector-buffered slot writer with digest verification and resume.
pub mod install;
/// Image manifest (`GET …/firmware/{version}/manifest`) parsing.
pub mod manifest;
/// Boot-count rollback and download-resume records (RTC persistent RAM).
pub mod policy;

#[cfg(test)]
mod http_tests;
#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod policy_tests;
