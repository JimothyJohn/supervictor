//! Stream an image into a slot and prove it is the one the manifest
//! describes before anyone boots it.
//!
//! The installer buffers to whole 4 KB sectors (one erase+program each),
//! hashes as it goes, checks the ESP-IDF header as soon as it has one, and
//! on `finish` compares size and digest, then reads the slot back and
//! hashes it again — a flash write that lied is caught here, not by the
//! bootloader refusing to boot. Progress is reported in whole sectors so a
//! reset mid-download resumes from flash that is really there.
//!
//! Generic over [`Slot`] so the whole thing runs on the host against an
//! in-memory slot with faults injected (`install_tests.rs`).

use sha2::{Digest, Sha256};
use supervictor_common::firmware::{self, ChipId, ImageError, ImageHeader};

use super::manifest::Manifest;

/// Flash sector: the erase granularity and the write unit used here.
pub const SECTOR: usize = 4096;

/// Where image bytes go. Offsets are relative to the slot.
pub trait Slot {
    /// Backend error.
    type Error: core::fmt::Debug;
    /// Bytes the slot can hold.
    fn capacity(&self) -> u32;
    /// Write `data` at `offset`. `offset` is sector aligned; `data` is a
    /// whole sector except possibly the image's last, shorter one.
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Self::Error>;
    /// Read `buf.len()` bytes from `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error>;
}

/// Why an install failed. `Slot` wraps the backend's error.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError<E> {
    /// The manifest's size exceeds this slot.
    TooLarge,
    /// More bytes arrived than the manifest's size.
    Overrun,
    /// `finish` was called before `size` bytes arrived.
    Truncated {
        /// Bytes received.
        got: u32,
        /// Bytes expected.
        want: u32,
    },
    /// The first bytes are not an image header for this chip.
    BadImage(ImageError),
    /// The header names a different chip than the manifest.
    ChipMismatch,
    /// The streamed bytes hash to something other than the manifest's digest.
    DigestMismatch,
    /// Flash, read back, hashes differently from what was streamed.
    ReadbackMismatch,
    /// Resume offset is not sector aligned or past the image.
    BadResume,
    /// Backend failure.
    Slot(E),
}

impl<E> From<E> for InstallError<E> {
    fn from(e: E) -> Self {
        InstallError::Slot(e)
    }
}

/// Proof that a slot holds the manifest's image, byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// Image size.
    pub size: u32,
    /// Image digest.
    pub sha256: [u8; 32],
}

/// Streaming writer for one image into one slot.
pub struct Installer<'a, S: Slot> {
    slot: S,
    buf: &'a mut [u8; SECTOR],
    buffered: usize,
    written: u32,
    size: u32,
    expected: [u8; 32],
    chip: ChipId,
    header_checked: bool,
    hasher: Sha256,
}

impl<'a, S: Slot> Installer<'a, S> {
    /// Start a fresh install of `manifest` into `slot`.
    pub fn new(
        slot: S,
        buf: &'a mut [u8; SECTOR],
        manifest: &Manifest,
    ) -> Result<Self, InstallError<S::Error>> {
        if manifest.size > slot.capacity() || !firmware::fits_slot(manifest.size) {
            return Err(InstallError::TooLarge);
        }
        Ok(Self {
            slot,
            buf,
            buffered: 0,
            written: 0,
            size: manifest.size,
            expected: manifest.sha256,
            chip: manifest.chip,
            header_checked: false,
            hasher: Sha256::new(),
        })
    }

    /// Continue an install whose first `offset` bytes are already in the
    /// slot (from a previous attempt of the *same* manifest). The prefix
    /// is read back and hashed so the final digest covers it; its header
    /// is re-checked too.
    pub fn resume(
        slot: S,
        buf: &'a mut [u8; SECTOR],
        manifest: &Manifest,
        offset: u32,
    ) -> Result<Self, InstallError<S::Error>> {
        let mut me = Self::new(slot, buf, manifest)?;
        if !(offset as usize).is_multiple_of(SECTOR) || offset > manifest.size {
            return Err(InstallError::BadResume);
        }
        let mut pos = 0u32;
        while pos < offset {
            let n = (offset - pos).min(SECTOR as u32) as usize;
            me.slot.read(pos, &mut me.buf[..n])?;
            if !me.header_checked && n >= firmware::HEADER_LEN {
                me.check_header()?;
            }
            me.hasher.update(&me.buf[..n]);
            pos += n as u32;
        }
        me.written = offset;
        Ok(me)
    }

    /// Bytes committed to the slot so far (sector aligned until `finish`).
    /// This is what a resume record should carry.
    pub fn committed(&self) -> u32 {
        self.written
    }

    /// Bytes received so far, committed or buffered.
    pub fn received(&self) -> u32 {
        self.written + self.buffered as u32
    }

    /// Bytes still expected.
    pub fn remaining(&self) -> u32 {
        self.size - self.received()
    }

    fn check_header(&mut self) -> Result<(), InstallError<S::Error>> {
        let header = ImageHeader::parse(&self.buf[..firmware::HEADER_LEN])
            .map_err(InstallError::BadImage)?;
        if header.chip != self.chip {
            return Err(InstallError::ChipMismatch);
        }
        self.header_checked = true;
        Ok(())
    }

    /// Accept the next bytes of the image, in any chunking. Returns the
    /// number of whole sectors committed to flash by this call, so the
    /// caller knows when to refresh its resume record.
    pub fn feed(&mut self, mut data: &[u8]) -> Result<u32, InstallError<S::Error>> {
        if data.len() as u64 > u64::from(self.remaining()) {
            return Err(InstallError::Overrun);
        }
        let mut sectors = 0;
        while !data.is_empty() {
            let room = SECTOR - self.buffered;
            let n = data.len().min(room);
            self.buf[self.buffered..self.buffered + n].copy_from_slice(&data[..n]);
            self.buffered += n;
            data = &data[n..];
            if !self.header_checked && self.buffered >= firmware::HEADER_LEN {
                self.check_header()?;
            }
            if self.buffered == SECTOR {
                self.flush_sector()?;
                sectors += 1;
            }
        }
        Ok(sectors)
    }

    fn flush_sector(&mut self) -> Result<(), InstallError<S::Error>> {
        let n = self.buffered;
        self.slot.write(self.written, &self.buf[..n])?;
        self.hasher.update(&self.buf[..n]);
        self.written += n as u32;
        self.buffered = 0;
        Ok(())
    }

    /// All bytes have arrived: commit the tail, compare size and digest,
    /// then read the whole slot back and hash it again.
    pub fn finish(mut self) -> Result<Verified, InstallError<S::Error>> {
        if self.received() != self.size {
            return Err(InstallError::Truncated {
                got: self.received(),
                want: self.size,
            });
        }
        if !self.header_checked {
            // Only reachable for images shorter than a header, which the
            // manifest check already excludes; keep the invariant anyway.
            self.check_header()?;
        }
        if self.buffered > 0 {
            self.flush_sector()?;
        }
        let streamed: [u8; 32] = core::mem::take(&mut self.hasher).finalize().into();
        if streamed != self.expected {
            return Err(InstallError::DigestMismatch);
        }
        // Read back: the flash must hold what was streamed.
        let mut hasher = Sha256::new();
        let mut pos = 0u32;
        while pos < self.size {
            let n = (self.size - pos).min(SECTOR as u32) as usize;
            self.slot.read(pos, &mut self.buf[..n])?;
            hasher.update(&self.buf[..n]);
            pos += n as u32;
        }
        let on_flash: [u8; 32] = hasher.finalize().into();
        if on_flash != self.expected {
            return Err(InstallError::ReadbackMismatch);
        }
        Ok(Verified {
            size: self.size,
            sha256: self.expected,
        })
    }

    /// Give the slot back (after an error) so the caller can retry.
    pub fn into_slot(self) -> S {
        self.slot
    }
}
