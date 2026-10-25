//! The firmware manifest the endpoint serves per `(device, version)`:
//! `{"version","size","sha256","chip","uploaded_at"}` (wire names in
//! `supervictor_common::fields::FW_*`). Parsed with serde-json-core into
//! heapless strings, then checked field by field; unknown keys are
//! skipped so the endpoint can grow the manifest without bricking the
//! parser on older boards.

use heapless::String as HString;
use serde::Deserialize;
use supervictor_common::firmware::{self, ChipId, Version};

/// Longest manifest body the device will read (the endpoint's is ~170 B).
pub const MAX_LEN: usize = 512;

/// Why a manifest was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    /// Not the JSON shape expected (or a string too long for its buffer).
    Json,
    /// `version` is not `MAJOR.MINOR.PATCH[-pre]`.
    Version,
    /// `size` does not fit a slot (or is smaller than an image header).
    Size,
    /// `sha256` is not 64 hex characters.
    Digest,
    /// `chip` is not one this fleet runs.
    Chip,
}

#[derive(Deserialize)]
struct Wire {
    version: HString<32>,
    size: u32,
    sha256: HString<64>,
    chip: HString<8>,
}

/// A validated manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Version string, parseable by [`Version::parse`].
    pub version: HString<32>,
    /// Image size in bytes; fits a slot.
    pub size: u32,
    /// SHA-256 of the whole image.
    pub sha256: [u8; 32],
    /// Chip the image targets.
    pub chip: ChipId,
}

impl Manifest {
    /// Parse and validate a manifest body.
    pub fn parse(body: &[u8]) -> Result<Manifest, ManifestError> {
        if body.len() > MAX_LEN {
            return Err(ManifestError::Json);
        }
        let (wire, _) = serde_json_core::from_slice::<Wire>(body).map_err(|_| ManifestError::Json)?;
        if Version::parse(wire.version.as_str()).is_none() {
            return Err(ManifestError::Version);
        }
        if !firmware::fits_slot(wire.size) {
            return Err(ManifestError::Size);
        }
        let sha256 = decode_hex32(wire.sha256.as_str()).ok_or(ManifestError::Digest)?;
        let chip = ChipId::parse(wire.chip.as_str()).ok_or(ManifestError::Chip)?;
        // heapless 0.9 String<N> -> String<N> of the same N: a plain copy.
        let mut version: HString<32> = HString::new();
        version
            .push_str(wire.version.as_str())
            .map_err(|_| ManifestError::Version)?;
        Ok(Manifest {
            version,
            size: wire.size,
            sha256,
            chip,
        })
    }

    /// The manifest's version, parsed.
    pub fn version(&self) -> Version<'_> {
        // Validated in `parse`; a manifest cannot exist with a bad version.
        Version::parse(self.version.as_str()).unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre: None,
        })
    }
}

/// 64 hex chars -> 32 bytes; `None` for any other length or character.
pub fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for (i, pair) in b.chunks_exact(2).enumerate() {
        out[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

/// Lowercase hex of a 32-byte digest (for `If-Match` and the console).
pub fn encode_hex32(d: &[u8; 32]) -> HString<64> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = HString::new();
    for b in d {
        let _ = s.push(HEX[usize::from(b >> 4)] as char);
        let _ = s.push(HEX[usize::from(b & 0xf)] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"version":"0.2.0","size":1085760,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32","uploaded_at":"2026-10-25T21:00:00+00:00"}"#;

    #[test]
    fn parses_the_endpoints_manifest_and_skips_unknown_keys() {
        let m = Manifest::parse(GOOD.as_bytes()).unwrap();
        assert_eq!(m.version.as_str(), "0.2.0");
        assert_eq!(m.size, 1_085_760);
        assert_eq!(m.chip, ChipId::Esp32);
        assert_eq!(m.sha256[0], 0xba);
        assert_eq!(m.sha256[31], 0xad);
        let at = GOOD.find("ba7816").unwrap();
        assert_eq!(encode_hex32(&m.sha256).as_str(), &GOOD[at..at + 64]);
        assert_eq!(m.version().minor, 2);
    }

    #[test]
    fn key_order_does_not_matter_and_future_keys_are_fine() {
        let body = r#"{"extra":{"nested":[1,2,3]},"chip":"esp32c3","sha256":"BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD","size":24,"version":"1.0.0-rc.1","flag":true}"#;
        let m = Manifest::parse(body.as_bytes()).unwrap();
        assert_eq!(m.chip, ChipId::Esp32C3);
        assert_eq!(m.version().pre, Some("rc.1"));
        assert_eq!(m.sha256[0], 0xba, "uppercase hex accepted");
    }

    #[test]
    fn rejects_each_bad_field() {
        let cases = [
            (r#"{"version":"0.2","size":100,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Version),
            (r#"{"version":"../0.2.0","size":100,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Version),
            (r#"{"version":"0.2.0","size":0,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Size),
            (r#"{"version":"0.2.0","size":2031617,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Size),
            (r#"{"version":"0.2.0","size":100,"sha256":"ba7816","chip":"esp32"}"#, ManifestError::Digest),
            (r#"{"version":"0.2.0","size":100,"sha256":"zz7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Digest),
            (r#"{"version":"0.2.0","size":100,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32s3"}"#, ManifestError::Chip),
            (r#"{"version":"0.2.0","size":100,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}"#, ManifestError::Json),
            (r#"{"version":"0.2.0","size":"100","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Json),
            (r#"{"version":"0.2.0","size":-5,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Json),
            ("", ManifestError::Json),
            ("[]", ManifestError::Json),
            ("null", ManifestError::Json),
            (r#"{"version":"0.2.0","size":100,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015adba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","chip":"esp32"}"#, ManifestError::Json),
        ];
        for (body, want) in cases {
            assert_eq!(Manifest::parse(body.as_bytes()), Err(want), "{body}");
        }
    }

    #[test]
    fn oversized_body_is_refused_before_parsing() {
        let mut big = heapless::Vec::<u8, 1024>::new();
        big.extend_from_slice(GOOD.as_bytes()).unwrap();
        while big.len() <= MAX_LEN {
            big.push(b' ').unwrap();
        }
        assert_eq!(Manifest::parse(&big), Err(ManifestError::Json));
    }

    #[test]
    fn hex_round_trips() {
        let d: [u8; 32] = core::array::from_fn(|i| (i * 7) as u8);
        let h = encode_hex32(&d);
        assert_eq!(decode_hex32(h.as_str()), Some(d));
        assert_eq!(decode_hex32(""), None);
        assert_eq!(decode_hex32(&"0".repeat(63)), None);
        assert_eq!(decode_hex32(&"0".repeat(65)), None);
        assert_eq!(decode_hex32(&"g".repeat(64)), None);
    }
}
