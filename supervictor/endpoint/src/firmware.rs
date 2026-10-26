//! Firmware images for OTA (docs/OTA.md): upload validation, the on-disk
//! blob store, and HTTP `Range` parsing for the device's chunked download.
//!
//! Images are keyed by `(device_id, version)` because every image bakes in
//! that device's certificate and key: one device's image is useless to —
//! and must never be served to — another. The store is content-addressed
//! on the side (`sha256` in the manifest, also the ETag) so the device can
//! verify what it wrote to flash before it reboots into it.
//!
//! Self-hosted the store is a directory (`FIRMWARE_DIR`); the Lambda path
//! has no blob store yet and answers 503 on these routes (TODO.md).

use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Bytes;

use sha2::{Digest, Sha256};
use supervictor_common::firmware::{self, ChipId, ImageHeader};
use supervictor_common::models::{FirmwareFault, FirmwareManifest};

use crate::error::AppError;
use crate::middleware::sanitize_for_log;
use crate::store::StoreError;

/// Largest request body the upload route accepts: one slot. Anything
/// bigger cannot be installed, so it is refused before it is read.
pub const MAX_IMAGE_BYTES: usize = firmware::SLOT_SIZE as usize;

/// Lowercase hex of a SHA-256 digest.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// What an uploaded image must satisfy before it is stored: an ESP-IDF
/// header for a chip this fleet runs, a size that fits a slot, and — when
/// the image says a digest is appended — a digest that matches, so the
/// bootloader will accept the bytes once they are in flash.
pub fn validate_image(bytes: &[u8]) -> Result<ImageHeader, AppError> {
    let invalid = |reason: &str| AppError::InvalidPayload {
        detail: format!("image: {reason}"),
        structured: Some(serde_json::json!({ "field": "image", "reason": reason })),
    };
    let header = ImageHeader::parse(bytes).map_err(|e| {
        invalid(match e {
            firmware::ImageError::TooShort => "too short to be an image",
            firmware::ImageError::BadMagic => "not an ESP-IDF image (bad magic)",
            firmware::ImageError::BadSegmentCount => "bad segment count",
            firmware::ImageError::UnknownChip(_) => "chip is not one this fleet runs",
        })
    })?;
    let len = u32::try_from(bytes.len()).map_err(|_| invalid("larger than a slot"))?;
    if !firmware::fits_slot(len) {
        return Err(invalid("larger than a slot"));
    }
    if header.hash_appended {
        if bytes.len() < firmware::HEADER_LEN + firmware::DIGEST_LEN {
            return Err(invalid("too short for its appended digest"));
        }
        let (body, tail) = bytes.split_at(bytes.len() - firmware::DIGEST_LEN);
        if Sha256::digest(body).as_slice() != tail {
            return Err(invalid("appended sha256 does not match the image"));
        }
    }
    Ok(header)
}

/// Where firmware blobs live and how they are read back.
pub trait FirmwareStore: Send + Sync {
    /// Store `bytes` as `version` for `device_id`, replacing any previous
    /// image of that version, and return its manifest. The caller has
    /// validated the image and the identifiers.
    fn put(
        &self,
        device_id: &str,
        version: &str,
        chip: ChipId,
        bytes: &[u8],
        uploaded_at: &str,
    ) -> Result<FirmwareManifest, AppError>;
    /// The manifest of a stored image, `None` when there is none.
    fn manifest(
        &self,
        device_id: &str,
        version: &str,
    ) -> Result<Option<FirmwareManifest>, AppError>;
    /// `len` bytes from `start` of the blob whose digest is `sha256` (as
    /// the manifest names it), or `None` when that blob is gone. Short
    /// when the range runs past the end. Naming the digest keeps a reader
    /// on the bytes its manifest describes even while a re-upload of the
    /// same version lands.
    fn read(
        &self,
        device_id: &str,
        version: &str,
        sha256: &str,
        start: u64,
        len: u64,
    ) -> Result<Option<Vec<u8>>, AppError>;
    /// Remove a stored image; `Ok(false)` when there was none.
    fn delete(&self, device_id: &str, version: &str) -> Result<bool, AppError>;
}

/// A device id that is safe as a directory name: the registration rules
/// (`validate::check_device_id`) plus a strict charset, so no separator,
/// dot-segment or shell surprise ever reaches the filesystem.
pub fn is_path_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= crate::validate::MAX_DEVICE_ID_LEN
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Reject identifiers the blob store cannot name safely. Versions are
/// checked with the same parser the device uses (`firmware::Version`).
pub fn check_identifiers(device_id: &str, version: &str) -> Result<(), AppError> {
    crate::validate::check_device_id(supervictor_common::fields::DEVICE_ID, device_id)?;
    if !is_path_safe_id(device_id) {
        return Err(AppError::InvalidPayload {
            detail: "device_id: not a valid firmware path component".into(),
            structured: Some(serde_json::json!({
                "field": supervictor_common::fields::DEVICE_ID,
                "reason": "only letters, digits, '-', '_' and '.' are allowed",
            })),
        });
    }
    if !firmware::is_valid_version(version) {
        return Err(AppError::InvalidPayload {
            detail: "version: not MAJOR.MINOR.PATCH[-pre]".into(),
            structured: Some(serde_json::json!({
                "field": supervictor_common::fields::FW_VERSION,
                "reason": "must be MAJOR.MINOR.PATCH with an optional -pre tag",
            })),
        });
    }
    Ok(())
}

/// Directory-backed [`FirmwareStore`]: `<root>/<device_id>/<version>.json`
/// is the manifest and `<version>-<sha256>.bin` the blob it names. The
/// blob is content-addressed, so the manifest a reader holds always
/// describes exactly the bytes it goes on to read, even while another
/// upload of the same version lands: that upload writes a *different*
/// blob and then swaps the manifest. Every file is written to a temporary
/// name and renamed into place. Blobs no manifest names any more are
/// swept on the next upload once they are a minute old — young enough to
/// be another admin's in-flight upload, they are left alone.
pub struct FsFirmwareStore {
    root: PathBuf,
    tmp_seq: AtomicU64,
}

/// Stale-blob sweep grace: a blob younger than this may be an upload that
/// has not swapped its manifest in yet.
const SWEEP_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

impl FsFirmwareStore {
    /// Open (creating) the store directory.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|e| AppError::Store(StoreError::io("firmware dir create", e)))?;
        Ok(Self {
            root,
            tmp_seq: AtomicU64::new(0),
        })
    }

    /// The store directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn device_dir(&self, device_id: &str, version: &str) -> Result<PathBuf, AppError> {
        // Belt and braces: the handlers validate first, but the store is
        // the last thing between a request and the filesystem.
        if !is_path_safe_id(device_id) || !firmware::is_valid_version(version) {
            return Err(AppError::InvalidPayload {
                detail: "firmware: unsafe identifier".into(),
                structured: None,
            });
        }
        Ok(self.root.join(device_id))
    }

    fn manifest_path(dir: &Path, version: &str) -> PathBuf {
        dir.join(format!("{version}.json"))
    }

    fn blob_path(dir: &Path, version: &str, sha256: &str) -> Option<PathBuf> {
        // The manifest is our own file, but a hand-edited one must not be
        // able to name a path: the digest is 64 hex chars or nothing.
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(dir.join(format!("{version}-{sha256}.bin")))
    }

    fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<(), AppError> {
        let io = |op: &'static str| move |e: std::io::Error| AppError::Store(StoreError::io(op, e));
        let dir = target
            .parent()
            .ok_or_else(|| AppError::Store(StoreError::io("firmware path", "no parent")))?;
        fs::create_dir_all(dir).map_err(io("firmware dir create"))?;
        let seq = self.tmp_seq.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(
            ".{}.tmp-{}-{seq}",
            target.file_name().and_then(|n| n.to_str()).unwrap_or("fw"),
            std::process::id()
        ));
        let result = (|| {
            let mut f = fs::File::create(&tmp).map_err(io("firmware tmp create"))?;
            f.write_all(bytes).map_err(io("firmware tmp write"))?;
            f.sync_all().map_err(io("firmware tmp sync"))?;
            fs::rename(&tmp, target).map_err(io("firmware rename"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Remove `<version>-*.bin` blobs other than `keep` that are older
    /// than [`SWEEP_GRACE`]. Best effort: a failed sweep is a leak, not
    /// an error for the upload that triggered it.
    fn sweep(dir: &Path, version: &str, keep: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let prefix = format!("{version}-");
        for entry in entries.flatten() {
            let path = entry.path();
            if path == keep {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !(name.starts_with(&prefix) && name.ends_with(".bin")) {
                continue;
            }
            let old_enough = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age >= SWEEP_GRACE);
            if old_enough {
                let _ = fs::remove_file(&path);
            }
        }
    }

    fn read_manifest(path: &Path) -> Result<Option<FirmwareManifest>, AppError> {
        match fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw)
                .map(Some)
                .map_err(|e| AppError::Store(StoreError::serde("firmware manifest read", e))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::Store(StoreError::io("firmware manifest read", e))),
        }
    }
}

impl FirmwareStore for FsFirmwareStore {
    fn put(
        &self,
        device_id: &str,
        version: &str,
        chip: ChipId,
        bytes: &[u8],
        uploaded_at: &str,
    ) -> Result<FirmwareManifest, AppError> {
        let dir = self.device_dir(device_id, version)?;
        let manifest = FirmwareManifest {
            version: version.to_string(),
            size: u32::try_from(bytes.len()).map_err(|_| AppError::InvalidPayload {
                detail: "image: larger than a slot".into(),
                structured: None,
            })?,
            sha256: sha256_hex(bytes),
            chip: chip.label().to_string(),
            uploaded_at: uploaded_at.to_string(),
        };
        let manifest_json = serde_json::to_vec(&manifest)
            .map_err(|e| AppError::Store(StoreError::serde("firmware manifest", e)))?;
        let blob = Self::blob_path(&dir, version, &manifest.sha256)
            .ok_or_else(|| AppError::Store(StoreError::io("firmware blob path", "bad digest")))?;
        // Blob first (content-addressed, so a concurrent identical upload
        // writing the same path is harmless), then the manifest swap.
        self.write_atomic(&blob, bytes)?;
        self.write_atomic(&Self::manifest_path(&dir, version), &manifest_json)?;
        Self::sweep(&dir, version, &blob);
        tracing::info!(
            device_id = %sanitize_for_log(device_id),
            version = %sanitize_for_log(version),
            size = manifest.size,
            sha256 = %manifest.sha256,
            "firmware stored"
        );
        Ok(manifest)
    }

    fn manifest(
        &self,
        device_id: &str,
        version: &str,
    ) -> Result<Option<FirmwareManifest>, AppError> {
        let dir = self.device_dir(device_id, version)?;
        Self::read_manifest(&Self::manifest_path(&dir, version))
    }

    fn read(
        &self,
        device_id: &str,
        version: &str,
        sha256: &str,
        start: u64,
        len: u64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let dir = self.device_dir(device_id, version)?;
        let Some(blob) = Self::blob_path(&dir, version, sha256) else {
            return Err(AppError::InvalidPayload {
                detail: "firmware: invalid digest".into(),
                structured: None,
            });
        };
        let mut f = match fs::File::open(&blob) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(AppError::Store(StoreError::io("firmware open", e))),
        };
        f.seek(SeekFrom::Start(start))
            .map_err(|e| AppError::Store(StoreError::io("firmware seek", e)))?;
        let mut out = Vec::with_capacity(len.min(MAX_IMAGE_BYTES as u64) as usize);
        f.take(len)
            .read_to_end(&mut out)
            .map_err(|e| AppError::Store(StoreError::io("firmware read", e)))?;
        Ok(Some(out))
    }

    fn delete(&self, device_id: &str, version: &str) -> Result<bool, AppError> {
        let dir = self.device_dir(device_id, version)?;
        // Manifest first so readers stop seeing the image before the bytes
        // go; then every blob of that version, stale ones included.
        let had_manifest = match fs::remove_file(Self::manifest_path(&dir, version)) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(AppError::Store(StoreError::io("firmware delete", e))),
        };
        let prefix = format!("{version}-");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_blob = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".bin"));
                if is_blob {
                    fs::remove_file(&path)
                        .map_err(|e| AppError::Store(StoreError::io("firmware delete", e)))?;
                }
            }
        }
        Ok(had_manifest)
    }
}

/// One satisfiable byte range of a `total`-byte resource: `start..=end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// First byte offset.
    pub start: u64,
    /// Last byte offset, inclusive.
    pub end: u64,
}

impl ByteRange {
    /// Bytes in the range.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Never empty: a satisfiable range has at least one byte.
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// How a `Range` header was understood (RFC 7233 §3.1 / §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOutcome {
    /// No header, a header in a unit or shape this server does not do
    /// (including multiple ranges): serve the whole resource with 200.
    Whole,
    /// One satisfiable range: serve 206.
    Partial(ByteRange),
    /// A well-formed single range that no byte of the resource satisfies:
    /// 416 with `Content-Range: bytes */total`.
    Unsatisfiable,
}

/// Interpret a `Range` header against a `total`-byte resource. Only a
/// single `bytes=` range is honoured — `start-end`, `start-` and `-suffix`
/// — which is all the device ever sends. Anything else is served whole
/// rather than refused: that is what the RFC allows and it keeps a client
/// with an odd header working. An empty resource has no satisfiable range.
pub fn parse_range(header: Option<&str>, total: u64) -> RangeOutcome {
    let Some(header) = header else {
        return RangeOutcome::Whole;
    };
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return RangeOutcome::Whole;
    };
    let spec = spec.trim();
    if spec.is_empty() || spec.contains(',') {
        return RangeOutcome::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeOutcome::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    let digits = |s: &str| -> Option<u64> {
        if s.is_empty() || s.len() > 19 || !s.bytes().all(|b| b.is_ascii_digit()) {
            None
        } else {
            s.parse().ok()
        }
    };
    if total == 0 {
        return RangeOutcome::Unsatisfiable;
    }
    match (first.is_empty(), last.is_empty()) {
        // "-suffix": the last `suffix` bytes.
        (true, false) => match digits(last) {
            Some(0) | None => RangeOutcome::Whole,
            Some(n) => {
                let start = total.saturating_sub(n);
                RangeOutcome::Partial(ByteRange {
                    start,
                    end: total - 1,
                })
            }
        },
        // "start-": from `start` to the end.
        (false, true) => match digits(first) {
            None => RangeOutcome::Whole,
            Some(start) if start >= total => RangeOutcome::Unsatisfiable,
            Some(start) => RangeOutcome::Partial(ByteRange {
                start,
                end: total - 1,
            }),
        },
        // "start-end", end clamped to the resource.
        (false, false) => match (digits(first), digits(last)) {
            (Some(start), Some(end)) => {
                if start > end {
                    return RangeOutcome::Whole;
                }
                if start >= total {
                    return RangeOutcome::Unsatisfiable;
                }
                RangeOutcome::Partial(ByteRange {
                    start,
                    end: end.min(total - 1),
                })
            }
            _ => RangeOutcome::Whole,
        },
        (true, true) => RangeOutcome::Whole,
    }
}

// ── Bench fault injection ───────────────────────────────────────────────

/// Armed one-shot download faults per device (`PUT /devices/{id}/firmware_fault`).
/// Process memory on purpose: a bench knob, never fleet state.
pub type FaultTable = Arc<Mutex<HashMap<String, FirmwareFault>>>;

/// A new, empty fault table.
pub fn fault_table() -> FaultTable {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Lock the table, surviving a poisoned mutex (the data is a plain map).
pub fn faults(table: &FaultTable) -> MutexGuard<'_, HashMap<String, FirmwareFault>> {
    table.lock().unwrap_or_else(|e| e.into_inner())
}

/// An image body that pauses once. `body` is the bytes of the range being
/// served, starting at image offset `start`; the pause lands at the fault's
/// absolute `stall_at` (before the first byte if the range starts after it,
/// after the last if it ends before it). Total length and order are
/// untouched, so `Content-Length` and the digest stay honest: only time is
/// injected, which is what makes the device's read timeout fire.
pub struct StalledBody {
    head: Option<Bytes>,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    tail: Option<Bytes>,
}

impl StalledBody {
    /// Split `body` (served from image offset `start`) at the fault.
    pub fn new(body: Bytes, start: u64, fault: FirmwareFault) -> Self {
        let cut = fault.stall_at.saturating_sub(start).min(body.len() as u64) as usize;
        let tail = body.slice(cut..);
        let head = body.slice(..cut);
        StalledBody {
            head: (!head.is_empty()).then_some(head),
            sleep: Some(Box::pin(tokio::time::sleep(Duration::from_secs(
                fault.stall_secs,
            )))),
            tail: (!tail.is_empty()).then_some(tail),
        }
    }
}

impl futures_core::Stream for StalledBody {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if let Some(head) = this.head.take() {
            return Poll::Ready(Some(Ok(head)));
        }
        if let Some(sleep) = this.sleep.as_mut() {
            match sleep.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => this.sleep = None,
            }
        }
        Poll::Ready(this.tail.take().map(Ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_shapes() {
        let t = 1000;
        assert_eq!(parse_range(None, t), RangeOutcome::Whole);
        assert_eq!(
            parse_range(Some("bytes=0-99"), t),
            RangeOutcome::Partial(ByteRange { start: 0, end: 99 })
        );
        assert_eq!(
            parse_range(Some("bytes=900-"), t),
            RangeOutcome::Partial(ByteRange {
                start: 900,
                end: 999
            })
        );
        assert_eq!(
            parse_range(Some("bytes=-100"), t),
            RangeOutcome::Partial(ByteRange {
                start: 900,
                end: 999
            })
        );
        assert_eq!(
            parse_range(Some("bytes=-5000"), t),
            RangeOutcome::Partial(ByteRange { start: 0, end: 999 })
        );
        assert_eq!(
            parse_range(Some("bytes=990-5000"), t),
            RangeOutcome::Partial(ByteRange {
                start: 990,
                end: 999
            })
        );
        assert_eq!(
            parse_range(Some("bytes=1000-"), t),
            RangeOutcome::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=1000-1001"), t),
            RangeOutcome::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=0-"), 0),
            RangeOutcome::Unsatisfiable
        );
    }

    #[test]
    fn odd_ranges_are_served_whole_not_refused() {
        for odd in [
            "apples",
            "bytes=",
            "bytes=-",
            "bytes=-0",
            "bytes=5-2",
            "bytes=0-9,20-29",
            "bytes=a-b",
            "items=0-9",
            "bytes=0-9;extra",
            "bytes= - ",
            "bytes=99999999999999999999-",
            "bytes=0x10-",
            "bytes=٣-٤",
        ] {
            assert_eq!(parse_range(Some(odd), 1000), RangeOutcome::Whole, "{odd:?}");
        }
    }

    #[test]
    fn path_safe_ids() {
        for ok in ["poe-iso-rack-02", "a", "A.b_c-9", "x".repeat(64).as_str()] {
            assert!(is_path_safe_id(ok), "{ok:?}");
        }
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "../x",
            "a b",
            "ä",
            "a\0",
            "x".repeat(65).as_str(),
            "dev:1",
            "dev*",
        ] {
            assert!(!is_path_safe_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn sha_hex_is_64_lowercase() {
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(h.len(), 64);
    }
}
