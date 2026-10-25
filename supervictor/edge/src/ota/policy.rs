//! What survives a reset: an eight-word record in RTC fast memory that
//! tells the next boot whether it is running an unconfirmed image (and
//! how many times it has tried), or how far a download got before the
//! board reset. Pure functions over `[u32; 8]`; the device module owns the
//! actual `#[ram(persistent)]` static.
//!
//! The record carries a checksum: RTC memory is random on a cold boot and
//! the macro's zeroing can be skipped by some resets, so anything that
//! does not check out reads as [`State::Idle`]. Reverting to a slot on the
//! strength of garbage would be worse than not reverting at all.

/// Words in the record.
pub const WORDS: usize = 8;
/// `"SVOT"` little-endian: marks a record this firmware wrote.
pub const MAGIC: u32 = 0x544f_5653;
/// Unconfirmed boots tolerated before the slot flips back. The RTC
/// watchdog resets a wedged board every `WATCHDOG_TIMEOUT_S` (5 min), so
/// a broken image costs at most ~15 minutes before the old one is back.
pub const MAX_PENDING_BOOTS: u32 = 3;

const KIND_IDLE: u32 = 0;
const KIND_PENDING: u32 = 1;
const KIND_RESUME: u32 = 2;

/// What the record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing in flight.
    Idle,
    /// A freshly activated image is being tried; `boots` times so far.
    Pending {
        /// Boots of the new image without a confirmed uplink.
        boots: u32,
        /// `ota_app_number` of the slot to return to.
        previous_slot: u8,
    },
    /// A download of the image with this digest prefix and size reached
    /// `offset` bytes (sector aligned) before the board reset.
    Resume {
        /// Bytes safely in flash.
        offset: u32,
        /// Image size from the manifest.
        size: u32,
        /// First eight bytes of the manifest's SHA-256.
        sha_prefix: [u8; 8],
    },
}

/// What the boot path should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootDecision {
    /// Confirmed firmware: carry on.
    Normal,
    /// Unconfirmed image, within budget: run, and confirm after the first
    /// uplink round-trip. `boots` includes this one.
    Confirming {
        /// Boots so far including this one.
        boots: u32,
    },
    /// Budget exhausted: switch otadata back to `previous_slot` and reset.
    Revert {
        /// `ota_app_number` to return to.
        previous_slot: u8,
    },
}

/// FNV-1a over the first seven words: cheap, no tables, no dep.
fn checksum(words: &[u32; WORDS]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for w in &words[..WORDS - 1] {
        for b in w.to_le_bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// Decode a record; anything that does not check out is [`State::Idle`].
pub fn load(words: &[u32; WORDS]) -> State {
    if words[0] != MAGIC || words[WORDS - 1] != checksum(words) {
        return State::Idle;
    }
    match words[1] {
        KIND_PENDING => {
            let boots = words[2];
            let previous_slot = words[3];
            if previous_slot > 15 || boots > MAX_PENDING_BOOTS + 1 {
                return State::Idle;
            }
            State::Pending {
                boots,
                previous_slot: previous_slot as u8,
            }
        }
        KIND_RESUME => {
            let offset = words[2];
            let size = words[3];
            if offset > size || !supervictor_common::firmware::fits_slot(size) || offset % 4096 != 0 {
                return State::Idle;
            }
            let mut sha_prefix = [0u8; 8];
            sha_prefix[..4].copy_from_slice(&words[4].to_le_bytes());
            sha_prefix[4..].copy_from_slice(&words[5].to_le_bytes());
            State::Resume {
                offset,
                size,
                sha_prefix,
            }
        }
        _ => State::Idle,
    }
}

/// Encode a state into the record.
pub fn store(words: &mut [u32; WORDS], state: State) {
    *words = [0; WORDS];
    words[0] = MAGIC;
    match state {
        State::Idle => words[1] = KIND_IDLE,
        State::Pending {
            boots,
            previous_slot,
        } => {
            words[1] = KIND_PENDING;
            words[2] = boots;
            words[3] = u32::from(previous_slot);
        }
        State::Resume {
            offset,
            size,
            sha_prefix,
        } => {
            words[1] = KIND_RESUME;
            words[2] = offset;
            words[3] = size;
            words[4] = u32::from_le_bytes([sha_prefix[0], sha_prefix[1], sha_prefix[2], sha_prefix[3]]);
            words[5] = u32::from_le_bytes([sha_prefix[4], sha_prefix[5], sha_prefix[6], sha_prefix[7]]);
        }
    }
    words[WORDS - 1] = checksum(words);
}

/// Decide what this boot does and what the record should say afterwards.
/// A `Resume` record is left alone (the download will pick it up); a
/// `Pending` record counts this boot.
pub fn on_boot(state: State) -> (BootDecision, State) {
    match state {
        State::Pending {
            boots,
            previous_slot,
        } => {
            let boots = boots.saturating_add(1);
            if boots > MAX_PENDING_BOOTS {
                (BootDecision::Revert { previous_slot }, State::Idle)
            } else {
                (
                    BootDecision::Confirming { boots },
                    State::Pending {
                        boots,
                        previous_slot,
                    },
                )
            }
        }
        other => (BootDecision::Normal, other),
    }
}

/// The resume record that matches a manifest, if the stored one does.
pub fn resume_offset(state: State, size: u32, sha256: &[u8; 32]) -> u32 {
    match state {
        State::Resume {
            offset,
            size: s,
            sha_prefix,
        } if s == size && sha_prefix == sha256[..8] => offset,
        _ => 0,
    }
}

/// The resume record for a download of `sha256`/`size` that has `offset`
/// bytes in flash.
pub fn resume_state(offset: u32, size: u32, sha256: &[u8; 32]) -> State {
    let mut sha_prefix = [0u8; 8];
    sha_prefix.copy_from_slice(&sha256[..8]);
    State::Resume {
        offset,
        size,
        sha_prefix,
    }
}
