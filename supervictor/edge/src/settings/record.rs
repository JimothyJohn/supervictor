//! The settings record as it sits in flash.
//!
//! One fixed 256-byte frame, little-endian, checksummed, written to one of
//! two slots a sector apart (`A` at offset 0, `B` at [`SLOT_STRIDE`]). A
//! write goes to the slot *not* holding the current record with a higher
//! sequence number, so the previous record survives a power loss
//! mid-write and the next boot picks whichever valid slot is newer. Pure
//! functions over byte slices; `device` does the flash I/O.
//!
//! ```text
//!   0  magic "SVST"      4  version (1)   6  reserved    8  seq
//!  12  ssid len + 32    45  password len + 64
//! 110  host len + 64   175  device_name len + 64
//! 240  uplink_interval_s   244  threshold_ma   248  config_version   252  checksum
//! ```

use heapless::String as HString;

use super::{HostName, Settings, HOST_MAX, NAME_MAX, PASSWORD_MAX, SSID_MAX};

/// Bytes in a record.
pub const LEN: usize = 256;
/// `"SVST"` little-endian: marks a record this firmware wrote.
pub const MAGIC: u32 = 0x5453_5653;
/// Layout version this firmware reads and writes.
pub const VERSION: u16 = 1;
/// Distance between the two slots: one flash sector, so rewriting one
/// never erases the other.
pub const SLOT_STRIDE: u32 = 4096;
/// Flash the two slots need: the smallest `nvs` partition that works.
pub const SPAN: u32 = 2 * SLOT_STRIDE;

const OFF_SEQ: usize = 8;
const OFF_SSID: usize = 12;
const OFF_PASSWORD: usize = OFF_SSID + 1 + SSID_MAX;
const OFF_HOST: usize = OFF_PASSWORD + 1 + PASSWORD_MAX;
const OFF_NAME: usize = OFF_HOST + 1 + HOST_MAX;
const OFF_INTERVAL: usize = OFF_NAME + 1 + NAME_MAX;
const OFF_THRESHOLD: usize = OFF_INTERVAL + 4;
const OFF_CONFIG_VERSION: usize = OFF_THRESHOLD + 4;
const OFF_CHECKSUM: usize = LEN - 4;
const _: () = assert!(OFF_CONFIG_VERSION + 4 == OFF_CHECKSUM);

/// Which of the two slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Offset 0 in the partition.
    A,
    /// Offset [`SLOT_STRIDE`].
    B,
}

impl Slot {
    /// Byte offset of the slot inside the partition.
    pub fn offset(self) -> u32 {
        match self {
            Slot::A => 0,
            Slot::B => SLOT_STRIDE,
        }
    }

    /// The slot the next write goes to.
    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

/// What the partition holds: the newest valid record, where it is and
/// its sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Slot the record was read from.
    pub slot: Slot,
    /// Its sequence number; the next write uses `seq + 1`.
    pub seq: u32,
    /// The settings.
    pub settings: Settings,
}

/// FNV-1a over the frame before the checksum word: cheap, no tables, no
/// dep. Each step is a bijection on the running hash, so any single
/// changed byte changes the result.
fn checksum(frame: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in &frame[..OFF_CHECKSUM] {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn put_u32(frame: &mut [u8; LEN], at: usize, v: u32) {
    frame[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn get_u32(frame: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], frame[at + 3]])
}

/// Length-prefixed string field: one length byte, then `cap` bytes.
fn put_str(frame: &mut [u8; LEN], at: usize, s: &str) {
    frame[at] = s.len() as u8;
    frame[at + 1..at + 1 + s.len()].copy_from_slice(s.as_bytes());
}

fn get_str(frame: &[u8], at: usize, cap: usize) -> Option<&str> {
    let len = usize::from(frame[at]);
    if len > cap {
        return None;
    }
    core::str::from_utf8(&frame[at + 1..at + 1 + len]).ok()
}

/// Write `settings` as record number `seq` into `frame`.
pub fn encode(settings: &Settings, seq: u32, frame: &mut [u8; LEN]) {
    frame.fill(0);
    put_u32(frame, 0, MAGIC);
    frame[4..6].copy_from_slice(&VERSION.to_le_bytes());
    put_u32(frame, OFF_SEQ, seq);
    put_str(frame, OFF_SSID, settings.ssid.as_str());
    put_str(frame, OFF_PASSWORD, settings.password.as_str());
    put_str(frame, OFF_HOST, settings.host.as_str());
    put_str(frame, OFF_NAME, settings.device_name.as_str());
    put_u32(frame, OFF_INTERVAL, settings.uplink_interval_s);
    put_u32(frame, OFF_THRESHOLD, settings.threshold_ma);
    put_u32(frame, OFF_CONFIG_VERSION, settings.config_version);
    let sum = checksum(frame);
    put_u32(frame, OFF_CHECKSUM, sum);
}

/// Read a record; `None` for anything that is not a complete, intact
/// frame this firmware understands.
pub fn decode(frame: &[u8]) -> Option<(u32, Settings)> {
    if frame.len() < LEN {
        return None;
    }
    let frame = &frame[..LEN];
    if get_u32(frame, 0) != MAGIC {
        return None;
    }
    if u16::from_le_bytes([frame[4], frame[5]]) != VERSION {
        return None;
    }
    if get_u32(frame, OFF_CHECKSUM) != checksum(frame) {
        return None;
    }
    let settings = Settings {
        ssid: HString::try_from(get_str(frame, OFF_SSID, SSID_MAX)?).ok()?,
        password: HString::try_from(get_str(frame, OFF_PASSWORD, PASSWORD_MAX)?).ok()?,
        host: HostName::new(get_str(frame, OFF_HOST, HOST_MAX)?)?,
        device_name: HString::try_from(get_str(frame, OFF_NAME, NAME_MAX)?).ok()?,
        uplink_interval_s: get_u32(frame, OFF_INTERVAL),
        threshold_ma: get_u32(frame, OFF_THRESHOLD),
        config_version: get_u32(frame, OFF_CONFIG_VERSION),
    };
    Some((get_u32(frame, OFF_SEQ), settings))
}

/// The newest valid record of the two slots. Equal sequence numbers
/// cannot happen from this firmware's writes; `A` wins if they do.
pub fn choose(a: &[u8], b: &[u8]) -> Option<Found> {
    let found = |slot: Slot, frame: &[u8]| {
        decode(frame).map(|(seq, settings)| Found {
            slot,
            seq,
            settings,
        })
    };
    match (found(Slot::A, a), found(Slot::B, b)) {
        (Some(a), Some(b)) => Some(if b.seq > a.seq { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Where and as what the next record is written, given what is there.
pub fn next(current: Option<&Found>) -> (Slot, u32) {
    match current {
        Some(f) => (f.slot.other(), f.seq.wrapping_add(1)),
        None => (Slot::A, 1),
    }
}
