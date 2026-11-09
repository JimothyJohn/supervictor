//! Runtime settings: what a board needs to join a network and find its
//! endpoint, kept in the `nvs` partition instead of baked into the image.
//!
//! Until 2026-11 the SSID, password, host and device name were `env!`
//! constants, so changing any of them meant a reflash. Now they are a
//! [`Settings`] record read from flash at boot ([`device`]); the values the
//! image was built with ([`Settings::baked`]) only seed an empty partition,
//! so a board already in the field keeps working across the OTA that
//! brings this in, and a bench image built without them boots
//! unprovisioned (the provisioning portal is the next step).
//!
//! The record itself ([`record`]) is a fixed 256-byte, checksummed layout
//! in two alternating slots: a write that loses power leaves the previous
//! slot intact. Nothing here is the ESP-IDF NVS format — no other code
//! reads that partition — and nothing here needs hardware to test.

use core::ffi::CStr;

use heapless::String as HString;

/// Flash glue: find the partition, read the slots, seed, write.
#[cfg(feature = "embedded")]
pub mod device;
/// The on-flash record: encode, decode, pick the newest slot.
pub mod record;

#[cfg(test)]
mod pushed_tests;
#[cfg(test)]
mod record_tests;

/// Longest SSID (802.11 limit).
pub const SSID_MAX: usize = 32;
/// Longest WPA passphrase (WPA2 limit is 63 characters).
pub const PASSWORD_MAX: usize = 64;
/// Longest endpoint hostname this firmware will dial.
pub const HOST_MAX: usize = 64;
/// Longest device id on the wire (`UplinkMessage` capacity).
pub const NAME_MAX: usize = 64;

/// Device id the image was built with: `DEVICE_NAME` (what the Quickstart
/// and `flash.sh` set per board, matching the certificate it bakes in),
/// else the historical bench id.
pub const BAKED_DEVICE_NAME: &str = match option_env!("DEVICE_NAME") {
    Some(name) => name,
    None => "1234567890",
};
/// Endpoint the image was built with (`HOST`), empty when unset.
pub const BAKED_HOST: &str = match option_env!("HOST") {
    Some(host) => host,
    None => "",
};
/// SSID the image was built with, empty when unset (wired builds).
pub const BAKED_SSID: &str = match option_env!("SSID") {
    Some(ssid) => ssid,
    None => "",
};
/// Passphrase the image was built with, empty when unset.
pub const BAKED_PASSWORD: &str = match option_env!("PASSWORD") {
    Some(password) => password,
    None => "",
};

// A baked value that does not fit is a build error, not a board that boots
// unprovisioned because a string was silently dropped.
const _: () = assert!(BAKED_DEVICE_NAME.len() <= NAME_MAX);
const _: () = assert!(BAKED_HOST.len() <= HOST_MAX);
const _: () = assert!(BAKED_SSID.len() <= SSID_MAX);
const _: () = assert!(BAKED_PASSWORD.len() <= PASSWORD_MAX);

/// A hostname kept NUL-terminated, because mbedtls wants the server name
/// as a `CStr` and the settings live for the life of the board.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HostName {
    bytes: [u8; HOST_MAX + 1],
    len: u8,
}

impl HostName {
    /// The empty hostname.
    pub const EMPTY: HostName = HostName {
        bytes: [0; HOST_MAX + 1],
        len: 0,
    };

    /// `None` when `s` is longer than [`HOST_MAX`] or contains a NUL.
    pub fn new(s: &str) -> Option<HostName> {
        if s.len() > HOST_MAX || s.bytes().any(|b| b == 0) {
            return None;
        }
        let mut bytes = [0u8; HOST_MAX + 1];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Some(HostName {
            bytes,
            len: s.len() as u8,
        })
    }

    /// The hostname.
    pub fn as_str(&self) -> &str {
        // Built from a &str in `new`; the bytes are never touched after.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }

    /// The hostname with its terminator, for the TLS server name.
    pub fn as_cstr(&self) -> &CStr {
        CStr::from_bytes_with_nul(&self.bytes[..usize::from(self.len) + 1]).unwrap_or(c"")
    }

    /// Nothing set.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl core::fmt::Debug for HostName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Default for HostName {
    fn default() -> Self {
        HostName::EMPTY
    }
}

/// What the board needs to know that is not in the image.
///
/// `uplink_interval_s`, `threshold_ma` and `config_version` are what the
/// endpoint pushes in the uplink response's `config` block
/// ([`Settings::apply_pushed`]); `0` means "the compiled default" / "no
/// block applied yet".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Settings {
    /// Network to join (WiFi builds); empty on wired boards.
    pub ssid: HString<SSID_MAX>,
    /// Passphrase for `ssid`; empty for an open network.
    pub password: HString<PASSWORD_MAX>,
    /// Endpoint hostname: DNS, TLS server name, HTTP `Host`.
    pub host: HostName,
    /// Device id on the wire. The certificate baked into the image is
    /// still the identity the endpoint trusts; this is what the uplink
    /// says, and the two match on every board the bench provisions.
    pub device_name: HString<NAME_MAX>,
    /// Seconds between uplinks; `0` = compiled default.
    pub uplink_interval_s: u32,
    /// Power-on threshold in mA; `0` = compiled default.
    pub threshold_ma: u32,
    /// Version of the pushed settings block these values came from
    /// (`supervictor_common::config::version`); `0` = none yet. Echoed in
    /// every uplink so the dashboard can tell pending from applied.
    pub config_version: u32,
}

impl Settings {
    /// The values the image was built with. Seeds an empty partition.
    pub fn baked() -> Settings {
        Settings {
            ssid: HString::try_from(BAKED_SSID).unwrap_or_default(),
            password: HString::try_from(BAKED_PASSWORD).unwrap_or_default(),
            host: HostName::new(BAKED_HOST).unwrap_or_default(),
            device_name: HString::try_from(BAKED_DEVICE_NAME).unwrap_or_default(),
            uplink_interval_s: 0,
            threshold_ma: 0,
            config_version: 0,
        }
    }

    /// Take a pushed settings block. `true` when it was new and the record
    /// changed (the caller persists); a block with version `0` or the
    /// version already held is nothing. The interval is clamped into
    /// what the firmware will run; a negative threshold reads as `0`.
    pub fn apply_pushed(
        &mut self,
        version: u32,
        uplink_interval_s: u32,
        threshold_ma: i64,
    ) -> bool {
        if version == 0 || version == self.config_version {
            return false;
        }
        self.config_version = version;
        self.uplink_interval_s = supervictor_common::config::clamp_interval_s(uplink_interval_s);
        self.threshold_ma = u32::try_from(threshold_ma.max(0)).unwrap_or(u32::MAX);
        true
    }

    /// Seconds between uplinks: the pushed value, else `compiled`.
    pub fn effective_interval_s(&self, compiled: u32) -> u32 {
        if self.uplink_interval_s == 0 {
            compiled
        } else {
            self.uplink_interval_s
        }
    }

    /// A WiFi build can try to join something.
    pub fn has_wifi(&self) -> bool {
        !self.ssid.is_empty()
    }

    /// The uplink has somewhere to go and something to call itself.
    pub fn has_endpoint(&self) -> bool {
        !self.host.is_empty() && !self.device_name.is_empty()
    }

    /// Nothing at all: a fresh partition on an image built without values.
    pub fn is_blank(&self) -> bool {
        self.ssid.is_empty()
            && self.password.is_empty()
            && self.host.is_empty()
            && self.device_name.is_empty()
            && self.uplink_interval_s == 0
            && self.threshold_ma == 0
            && self.config_version == 0
    }
}
