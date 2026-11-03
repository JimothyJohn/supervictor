//! Whether this boot runs the portal, decided from what the previous boot
//! left in RTC memory. Pure over a two-word record; `mod.rs` owns the
//! `#[ram(persistent)]` static on embedded builds.
//!
//! A station-mode boot writes [`Mark::Booted`] first thing and
//! [`Mark::Joined`] on its first association. The RTC watchdog resets a
//! board that never completes an uplink within `WATCHDOG_TIMEOUT_S`
//! (5 min); if the record still says `Booted` then, the board never got
//! on the network in that time and the next boot opens the portal. The
//! portal writes [`Mark::Portal`] so its own reset — after a save or an
//! idle timeout — goes back to a station attempt. A power-on / EN reset
//! zeroes RTC memory, which reads as "try the station first": a board
//! that was merely unplugged does not come back as an access point.

/// Words in the record.
pub const WORDS: usize = 2;
/// `"SVPT"` little-endian: marks a record this firmware wrote.
pub const MAGIC: u32 = 0x5450_5653;

/// What the previous boot left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Station boot started; no association yet.
    Booted,
    /// Station boot associated with the configured network.
    Joined,
    /// The portal ran.
    Portal,
}

impl Mark {
    fn code(self) -> u32 {
        match self {
            Mark::Booted => 1,
            Mark::Joined => 2,
            Mark::Portal => 3,
        }
    }

    fn from_code(c: u32) -> Option<Mark> {
        Some(match c {
            1 => Mark::Booted,
            2 => Mark::Joined,
            3 => Mark::Portal,
            _ => return None,
        })
    }
}

/// Decode; anything not written by this firmware is `None`.
pub fn load(words: &[u32; WORDS]) -> Option<Mark> {
    if words[0] != MAGIC {
        return None;
    }
    Mark::from_code(words[1])
}

/// Encode.
pub fn store(words: &mut [u32; WORDS], mark: Mark) {
    words[0] = MAGIC;
    words[1] = mark.code();
}

/// Why the portal is running, for the console and the idle policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// No SSID anywhere: nothing to try.
    Unprovisioned,
    /// A network is configured but the last boot never joined it.
    JoinFailed,
}

/// Open the portal this boot? `has_wifi` is whether settings hold an SSID.
pub fn decide(previous: Option<Mark>, has_wifi: bool) -> Option<Reason> {
    if !has_wifi {
        return Some(Reason::Unprovisioned);
    }
    match previous {
        Some(Mark::Booted) => Some(Reason::JoinFailed),
        _ => None,
    }
}
