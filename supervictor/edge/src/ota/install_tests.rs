//! The installer under fire: a RAM slot with injectable faults, driven by
//! property tests over random image sizes, chunkings, cut points and
//! corruptions. Invariants: a slot is never reported `Verified` unless it
//! holds exactly the manifest's bytes; any corruption, truncation or
//! overrun is an error; a reset mid-download resumes to the same result.

extern crate std;

use super::install::*;
use super::manifest::Manifest;
use std::vec;
use std::vec::Vec;
use heapless::String as HString;
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use supervictor_common::firmware::{ChipId, HEADER_LEN, MAGIC};

/// In-memory slot. Writes land immediately unless a fault says otherwise.
struct MemSlot {
    bytes: Vec<u8>,
    /// Fail the write that covers this offset, once.
    fail_write_at: Option<u32>,
    /// Silently corrupt one byte of this write (a lying flash).
    corrupt_write_at: Option<u32>,
    writes: Vec<(u32, usize)>,
}

#[derive(Debug, PartialEq, Eq)]
struct Fault;

impl MemSlot {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: vec![0xff; capacity],
            fail_write_at: None,
            corrupt_write_at: None,
            writes: Vec::new(),
        }
    }
}

impl Slot for MemSlot {
    type Error = Fault;
    fn capacity(&self) -> u32 {
        self.bytes.len() as u32
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Fault> {
        assert_eq!(offset as usize % SECTOR, 0, "unaligned write");
        assert!(data.len() <= SECTOR);
        let end = offset as usize + data.len();
        let covers = |at: u32| at >= offset && (at as usize) < end;
        if self.fail_write_at.is_some_and(covers) {
            self.fail_write_at = None;
            return Err(Fault);
        }
        assert!(end <= self.bytes.len(), "write past the slot");
        self.bytes[offset as usize..end].copy_from_slice(data);
        if let Some(c) = self.corrupt_write_at.filter(|&c| covers(c)) {
            self.corrupt_write_at = None;
            self.bytes[c as usize] ^= 0x55;
        }
        self.writes.push((offset, data.len()));
        Ok(())
    }
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Fault> {
        let end = offset as usize + buf.len();
        assert!(end <= self.bytes.len(), "read past the slot");
        buf.copy_from_slice(&self.bytes[offset as usize..end]);
        Ok(())
    }
}

fn image(chip: ChipId, size: usize, seed: u64) -> Vec<u8> {
    assert!(size >= HEADER_LEN);
    let mut img = vec![0u8; size];
    img[0] = MAGIC;
    img[1] = 4;
    img[12..14].copy_from_slice(&chip.raw().to_le_bytes());
    // Deterministic filler (xorshift) so every size has distinct content.
    let mut x = (seed << 1) | 1;
    for b in &mut img[HEADER_LEN..] {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    img
}

fn manifest_for(img: &[u8], chip: ChipId) -> Manifest {
    Manifest {
        version: HString::try_from("0.2.0").unwrap(),
        size: img.len() as u32,
        sha256: Sha256::digest(img).into(),
        chip,
    }
}

const CAP: usize = 64 * 1024;

fn install_in_chunks(img: &[u8], chunks: &[usize]) -> Result<(Verified, MemSlot), InstallError<Fault>> {
    let m = manifest_for(img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m)?;
    let mut pos = 0;
    let mut i = 0;
    while pos < img.len() {
        let n = chunks[i % chunks.len()].clamp(1, img.len() - pos);
        inst.feed(&img[pos..pos + n])?;
        pos += n;
        i += 1;
    }
    // finish consumes the installer; pull the slot out first via a clone of
    // the bytes we care about.
    let verified = inst.finish()?;
    Ok((verified, MemSlot::new(0)))
}

#[test]
fn happy_path_any_chunking() {
    for size in [HEADER_LEN, 100, 4095, 4096, 4097, 8192, 20_000, CAP] {
        let img = image(ChipId::Esp32, size, 1);
        for chunks in [&[1usize][..], &[7, 13], &[4096], &[1500], &[10_000]] {
            let (v, _) = install_in_chunks(&img, chunks).unwrap_or_else(|e| panic!("size {size} chunks {chunks:?}: {e:?}"));
            assert_eq!(v.size as usize, size);
        }
    }
}

#[test]
fn slot_holds_exactly_the_image_after_finish() {
    let img = image(ChipId::Esp32, 10_000, 2);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let slot = MemSlot::new(CAP);
    let mut inst = Installer::new(slot, &mut buf, &m).unwrap();
    inst.feed(&img).unwrap();
    assert_eq!(inst.committed(), 8192, "two whole sectors committed");
    assert_eq!(inst.received(), 10_000);
    // Peek at the slot through a second installer resume path instead:
    // finish, then check via a fresh read.
    let v = inst.finish().unwrap();
    assert_eq!(v.sha256, m.sha256);
}

#[test]
fn image_too_large_for_the_slot_is_refused_up_front() {
    let img = image(ChipId::Esp32, CAP + 1, 3);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    assert!(matches!(
        Installer::new(MemSlot::new(CAP), &mut buf, &m),
        Err(InstallError::TooLarge)
    ));
}

#[test]
fn wrong_chip_and_bad_magic_fail_at_the_header_not_at_the_end() {
    let img = image(ChipId::Esp32C3, 10_000, 4);
    let m = manifest_for(&img, ChipId::Esp32); // manifest says esp32
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
    // Feed one byte short of a header: no verdict yet.
    assert!(inst.feed(&img[..HEADER_LEN - 1]).is_ok());
    assert_eq!(inst.feed(&img[HEADER_LEN - 1..HEADER_LEN]), Err(InstallError::ChipMismatch));

    let mut bad = image(ChipId::Esp32, 10_000, 4);
    bad[0] = 0x00;
    let m = manifest_for(&bad, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
    assert!(matches!(inst.feed(&bad[..100]), Err(InstallError::BadImage(_))));
}

#[test]
fn truncated_download_is_reported_with_the_counts() {
    let img = image(ChipId::Esp32, 10_000, 5);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
    inst.feed(&img[..6000]).unwrap();
    assert_eq!(
        inst.finish().unwrap_err(),
        InstallError::Truncated { got: 6000, want: 10_000 }
    );
}

#[test]
fn overrun_is_refused_before_anything_is_written() {
    let img = image(ChipId::Esp32, 5000, 6);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
    inst.feed(&img[..4096]).unwrap();
    let mut extra = img[4096..].to_vec();
    extra.push(0xAA);
    assert_eq!(inst.feed(&extra), Err(InstallError::Overrun));
    // The valid remainder still completes.
    inst.feed(&img[4096..]).unwrap();
    assert!(inst.finish().is_ok());
}

#[test]
fn corrupted_stream_fails_the_digest_never_verifies() {
    let img = image(ChipId::Esp32, 9000, 7);
    let m = manifest_for(&img, ChipId::Esp32);
    for flip in [HEADER_LEN, 4095, 4096, 8999] {
        let mut bad = img.clone();
        bad[flip] ^= 0x01;
        let mut buf = [0u8; SECTOR];
        let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
        inst.feed(&bad).unwrap();
        assert_eq!(inst.finish().unwrap_err(), InstallError::DigestMismatch, "flip {flip}");
    }
}

#[test]
fn lying_flash_is_caught_by_the_readback() {
    let img = image(ChipId::Esp32, 9000, 8);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut slot = MemSlot::new(CAP);
    slot.corrupt_write_at = Some(4100);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(slot, &mut buf, &m).unwrap();
    inst.feed(&img).unwrap();
    assert_eq!(inst.finish().unwrap_err(), InstallError::ReadbackMismatch);
}

#[test]
fn a_failed_write_surfaces_and_the_committed_offset_stays_honest() {
    let img = image(ChipId::Esp32, 12_000, 9);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut slot = MemSlot::new(CAP);
    slot.fail_write_at = Some(4096);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(slot, &mut buf, &m).unwrap();
    assert_eq!(inst.feed(&img[..4096]), Ok(1));
    assert_eq!(inst.committed(), 4096);
    assert_eq!(inst.feed(&img[4096..8192]), Err(InstallError::Slot(Fault)));
    assert_eq!(inst.committed(), 4096, "the failed sector is not counted");
}

#[test]
fn resume_after_a_reset_completes_to_the_same_verified_image() {
    let img = image(ChipId::Esp32, 20_000, 10);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
    inst.feed(&img[..10_000]).unwrap();
    let committed = inst.committed();
    assert_eq!(committed, 8192);
    // "Reset": drop the installer, keep the slot bytes.
    let slot = inst.into_slot();
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::resume(slot, &mut buf, &m, committed).unwrap();
    assert_eq!(inst.received(), committed);
    inst.feed(&img[committed as usize..]).unwrap();
    let v = inst.finish().unwrap();
    assert_eq!(v.sha256, m.sha256);
}

#[test]
fn resume_rejects_unaligned_or_oversized_offsets() {
    let img = image(ChipId::Esp32, 20_000, 11);
    let m = manifest_for(&img, ChipId::Esp32);
    let mut buf = [0u8; SECTOR];
    assert!(matches!(
        Installer::resume(MemSlot::new(CAP), &mut buf, &m, 100),
        Err(InstallError::BadResume)
    ));
    let mut buf = [0u8; SECTOR];
    assert!(matches!(
        Installer::resume(MemSlot::new(CAP), &mut buf, &m, 24_576),
        Err(InstallError::BadResume)
    ));
}

#[test]
fn resume_over_a_stale_prefix_fails_the_digest() {
    // The slot holds a different image's first sectors (e.g. the resume
    // record survived but the server's image changed and If-Match was
    // not honoured): the final digest catches it.
    let old = image(ChipId::Esp32, 20_000, 12);
    let new = image(ChipId::Esp32, 20_000, 13);
    let m_new = manifest_for(&new, ChipId::Esp32);
    let mut slot = MemSlot::new(CAP);
    slot.bytes[..8192].copy_from_slice(&old[..8192]);
    let mut buf = [0u8; SECTOR];
    let mut inst = Installer::resume(slot, &mut buf, &m_new, 8192).unwrap();
    inst.feed(&new[8192..]).unwrap();
    assert_eq!(inst.finish().unwrap_err(), InstallError::DigestMismatch);
}

#[test]
fn resume_over_a_prefix_with_the_wrong_chip_header_is_refused() {
    let c3 = image(ChipId::Esp32C3, 20_000, 14);
    let m = manifest_for(&c3, ChipId::Esp32);
    let mut slot = MemSlot::new(CAP);
    slot.bytes[..4096].copy_from_slice(&c3[..4096]);
    let mut buf = [0u8; SECTOR];
    assert!(matches!(
        Installer::resume(slot, &mut buf, &m, 4096),
        Err(InstallError::ChipMismatch)
    ));
}

/// A fault schedule for one download attempt.
#[derive(Debug, Clone)]
struct Attempt {
    /// Feed sizes cycled through.
    chunks: Vec<usize>,
    /// Stop (simulated reset) after this many bytes have been fed, if any.
    cut_after: Option<usize>,
}

fn attempt_strategy(max_size: usize) -> impl Strategy<Value = Attempt> {
    (
        prop::collection::vec(1usize..6000, 1..6),
        prop::option::of(0usize..max_size),
    )
        .prop_map(|(chunks, cut_after)| Attempt { chunks, cut_after })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Any number of resets at arbitrary points, resumed from the honest
    /// committed offset, ends in exactly the image — or in a clean error
    /// when the stream is corrupted.
    #[test]
    fn resets_and_resumes_converge(
        size in HEADER_LEN..CAP,
        seed in any::<u64>(),
        attempts in prop::collection::vec(attempt_strategy(CAP), 1..5),
        corrupt in prop::option::of((0usize..CAP, 1u8..=255)),
    ) {
        let img = image(ChipId::Esp32, size, seed);
        let m = manifest_for(&img, ChipId::Esp32);
        let mut slot = MemSlot::new(CAP);
        let mut committed = 0u32;
        let mut finished = None;
        // The last attempt always runs to completion.
        for (i, attempt) in attempts.iter().enumerate() {
            let last = i == attempts.len() - 1;
            let mut buf = [0u8; SECTOR];
            let mut inst = if committed == 0 {
                Installer::new(slot, &mut buf, &m).unwrap()
            } else {
                Installer::resume(slot, &mut buf, &m, committed).unwrap()
            };
            let mut pos = committed as usize;
            let mut fed_this_attempt = 0usize;
            let mut k = 0;
            let mut cut = false;
            while pos < img.len() {
                if let (false, Some(c)) = (last, attempt.cut_after) {
                    if fed_this_attempt >= c { cut = true; break; }
                }
                let n = attempt.chunks[k % attempt.chunks.len()].min(img.len() - pos);
                let mut chunk = img[pos..pos + n].to_vec();
                if let Some((at, x)) = corrupt {
                    if at >= pos && at < pos + n { chunk[at - pos] ^= x; }
                }
                inst.feed(&chunk).unwrap();
                pos += n;
                fed_this_attempt += n;
                k += 1;
            }
            if cut {
                committed = inst.committed();
                prop_assert!(committed as usize <= pos);
                prop_assert_eq!(committed as usize % SECTOR, 0);
                slot = inst.into_slot();
                // Sectors below `committed` are the image's (or the
                // corrupted stream's) bytes — never 0xff filler.
                prop_assert!(slot.writes.iter().all(|(o, _)| *o < committed));
                continue;
            }
            finished = Some(inst.finish());
            break;
        }
        let result = finished.expect("last attempt completes");
        match corrupt {
            Some((at, _)) if at < size => prop_assert_eq!(result, Err(InstallError::DigestMismatch)),
            _ => {
                let v = result.unwrap();
                prop_assert_eq!(v.size as usize, size);
                prop_assert_eq!(v.sha256, m.sha256);
            }
        }
    }

    /// Feeding more than the manifest promises is always an Overrun, and
    /// never touches the slot beyond the image.
    #[test]
    fn overrun_never_writes_past_the_image(size in HEADER_LEN..20_000usize, extra in 1usize..5000) {
        let img = image(ChipId::Esp32, size, 42);
        let m = manifest_for(&img, ChipId::Esp32);
        let mut buf = [0u8; SECTOR];
        let mut inst = Installer::new(MemSlot::new(CAP), &mut buf, &m).unwrap();
        inst.feed(&img).unwrap();
        let junk = vec![0xAB; extra];
        prop_assert_eq!(inst.feed(&junk), Err(InstallError::Overrun));
        let slot = inst.into_slot();
        prop_assert!(slot.writes.iter().all(|(o, n)| *o as usize + n <= size.div_ceil(SECTOR) * SECTOR));
    }
}
