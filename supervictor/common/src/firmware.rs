//! Firmware image facts shared by the endpoint (upload validation) and the
//! device (OTA install): the ESP-IDF application image header, the A/B slot
//! size, and a version order for anti-rollback. `no_std`, no deps, so the
//! device and the cloud reject the same images for the same reasons.
//!
//! The image format is the ESP-IDF one that `espflash save-image` emits
//! (<https://docs.espressif.com/projects/esptool/en/latest/esp32/advanced-topics/firmware-image-format.html>):
//! a 24-byte header, then segments, then an optional appended SHA-256 of
//! everything before it when `hash_appended` is set. The second-stage
//! bootloader verifies that digest on every boot, so a torn or corrupted
//! slot never runs — but it also never boots, so the device checks the
//! same digest *before* activating a slot and only then reboots into it.

/// Bytes in the fixed image header.
pub const HEADER_LEN: usize = 24;
/// First header byte of every ESP-IDF image.
pub const MAGIC: u8 = 0xE9;
/// Length of the SHA-256 digest appended to a hashed image.
pub const DIGEST_LEN: usize = 32;
/// Size of each OTA app slot in `supervictor/edge/partitions.csv`
/// (`ota_0` / `ota_1`): the largest image a device can install.
pub const SLOT_SIZE: u32 = 0x1F_0000;
/// Most segments an image may declare (esptool's limit).
pub const MAX_SEGMENTS: u8 = 16;

/// Chip identifiers the image header carries (`esp_chip_id_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipId {
    /// Classic ESP32 (the PoE boards; Xtensa).
    Esp32,
    /// ESP32-C3 (riscv devkits).
    Esp32C3,
}

impl ChipId {
    /// Wire value in the image header (`esp_chip_id_t`).
    pub const fn raw(self) -> u16 {
        match self {
            ChipId::Esp32 => 0x0000,
            ChipId::Esp32C3 => 0x0005,
        }
    }

    /// Chip for a raw header value; `None` for chips this fleet never runs.
    pub const fn from_raw(raw: u16) -> Option<ChipId> {
        match raw {
            0x0000 => Some(ChipId::Esp32),
            0x0005 => Some(ChipId::Esp32C3),
            _ => None,
        }
    }

    /// Manifest / API token (`esp32`, `esp32c3`).
    pub const fn label(self) -> &'static str {
        match self {
            ChipId::Esp32 => "esp32",
            ChipId::Esp32C3 => "esp32c3",
        }
    }

    /// Parse a manifest token; anything else is `None`.
    pub fn parse(token: &str) -> Option<ChipId> {
        match token {
            "esp32" => Some(ChipId::Esp32),
            "esp32c3" => Some(ChipId::Esp32C3),
            _ => None,
        }
    }
}

/// Why an image header was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// Fewer than [`HEADER_LEN`] bytes.
    TooShort,
    /// First byte is not [`MAGIC`].
    BadMagic,
    /// Zero or more than [`MAX_SEGMENTS`] segments.
    BadSegmentCount,
    /// A chip this fleet does not run.
    UnknownChip(u16),
}

/// The parts of the image header the fleet cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageHeader {
    /// Chip the image was built for.
    pub chip: ChipId,
    /// Declared segment count.
    pub segments: u8,
    /// Whether a SHA-256 digest is appended to the image.
    pub hash_appended: bool,
}

impl ImageHeader {
    /// Parse the fixed header at the start of an image.
    pub fn parse(bytes: &[u8]) -> Result<ImageHeader, ImageError> {
        if bytes.len() < HEADER_LEN {
            return Err(ImageError::TooShort);
        }
        if bytes[0] != MAGIC {
            return Err(ImageError::BadMagic);
        }
        let segments = bytes[1];
        if segments == 0 || segments > MAX_SEGMENTS {
            return Err(ImageError::BadSegmentCount);
        }
        let raw_chip = u16::from_le_bytes([bytes[12], bytes[13]]);
        let chip = ChipId::from_raw(raw_chip).ok_or(ImageError::UnknownChip(raw_chip))?;
        Ok(ImageHeader {
            chip,
            segments,
            hash_appended: bytes[23] == 1,
        })
    }
}

/// Whether an image of `size` bytes fits an OTA slot. Zero and anything
/// shorter than a header are rejected too: they are not images.
pub const fn fits_slot(size: u32) -> bool {
    size >= HEADER_LEN as u32 && size <= SLOT_SIZE
}

/// A firmware version for anti-rollback ordering: `MAJOR.MINOR.PATCH`
/// with an optional `-pre` tag, as Cargo writes `CARGO_PKG_VERSION` and
/// `FW_VERSION` is expected to follow. Ordering is numeric per component;
/// a pre-release sorts *before* its release (`1.2.0-rc1 < 1.2.0`) and two
/// pre-releases of the same triple compare their tags bytewise. Build
/// metadata (`+…`) is not accepted: it would make two different images
/// compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version<'a> {
    /// Major component.
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
    /// Pre-release tag without the dash, if any.
    pub pre: Option<&'a str>,
}

impl<'a> Version<'a> {
    /// Longest version string accepted (the device's `fw` field is 16
    /// bytes; the manifest allows a little more for pre-release tags).
    pub const MAX_LEN: usize = 32;

    /// Parse `MAJOR.MINOR.PATCH[-pre]`; `None` for anything else, including
    /// leading zeros, empty components, whitespace and overlong input.
    pub fn parse(s: &'a str) -> Option<Version<'a>> {
        if s.is_empty() || s.len() > Self::MAX_LEN {
            return None;
        }
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        if let Some(pre) = pre {
            if pre.is_empty()
                || !pre
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return None;
            }
        }
        let mut parts = core.split('.');
        let major = component(parts.next()?)?;
        let minor = component(parts.next()?)?;
        let patch = component(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// True when `self` is strictly newer than `other`.
    pub fn is_newer_than(&self, other: &Version<'_>) -> bool {
        self.cmp(other) == core::cmp::Ordering::Greater
    }
}

/// One numeric component: ASCII digits only, no leading zero unless `0`,
/// at most 9 digits so it can never overflow a `u32`.
fn component(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    s.parse().ok()
}

impl PartialOrd for Version<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version<'_> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            },
            ord => ord,
        }
    }
}

/// Validate a version string as it may appear in a manifest, an API path
/// or a filename: parseable by [`Version::parse`], so it is also free of
/// path separators, whitespace and control characters.
pub fn is_valid_version(s: &str) -> bool {
    Version::parse(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(chip: u16, segments: u8, hash_appended: u8) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        h[0] = MAGIC;
        h[1] = segments;
        h[12..14].copy_from_slice(&chip.to_le_bytes());
        h[23] = hash_appended;
        h
    }

    #[test]
    fn parses_the_header_espflash_wrote_for_the_poe_iso() {
        // First 24 bytes of a real `espflash save-image --chip esp32` output
        // (debug espnow build, 2026-10-25): 5 segments, chip 0, hash appended.
        let bytes: [u8; HEADER_LEN] = [
            0xe9, 0x05, 0x02, 0x20, 0xc4, 0x10, 0x08, 0x40, 0xee, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x8f, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01,
        ];
        let h = ImageHeader::parse(&bytes).unwrap();
        assert_eq!(h.chip, ChipId::Esp32);
        assert_eq!(h.segments, 5);
        assert!(h.hash_appended);
    }

    #[test]
    fn rejects_short_wrong_magic_and_bad_segment_counts() {
        assert_eq!(ImageHeader::parse(&[]), Err(ImageError::TooShort));
        assert_eq!(
            ImageHeader::parse(&header(0, 1, 1)[..HEADER_LEN - 1]),
            Err(ImageError::TooShort)
        );
        let mut h = header(0, 1, 1);
        h[0] = 0xE8;
        assert_eq!(ImageHeader::parse(&h), Err(ImageError::BadMagic));
        assert_eq!(
            ImageHeader::parse(&header(0, 0, 1)),
            Err(ImageError::BadSegmentCount)
        );
        assert_eq!(
            ImageHeader::parse(&header(0, 17, 1)),
            Err(ImageError::BadSegmentCount)
        );
    }

    #[test]
    fn rejects_chips_the_fleet_does_not_run() {
        // ESP32-S3 is 0x0009: a real chip, not ours.
        assert_eq!(
            ImageHeader::parse(&header(0x0009, 3, 1)),
            Err(ImageError::UnknownChip(9))
        );
        assert_eq!(
            ImageHeader::parse(&header(5, 3, 0)).unwrap().chip,
            ChipId::Esp32C3
        );
        assert!(!ImageHeader::parse(&header(5, 3, 0)).unwrap().hash_appended);
        // Anything but exactly 1 is "no hash" — the bootloader reads it the same way.
        assert!(!ImageHeader::parse(&header(0, 3, 2)).unwrap().hash_appended);
    }

    #[test]
    fn chip_tokens_round_trip() {
        for chip in [ChipId::Esp32, ChipId::Esp32C3] {
            assert_eq!(ChipId::parse(chip.label()), Some(chip));
            assert_eq!(ChipId::from_raw(chip.raw()), Some(chip));
        }
        assert_eq!(ChipId::parse("esp32s3"), None);
        assert_eq!(ChipId::parse("ESP32"), None);
    }

    #[test]
    fn slot_fit_bounds() {
        assert!(!fits_slot(0));
        assert!(!fits_slot(HEADER_LEN as u32 - 1));
        assert!(fits_slot(HEADER_LEN as u32));
        assert!(fits_slot(SLOT_SIZE));
        assert!(!fits_slot(SLOT_SIZE + 1));
        assert!(!fits_slot(u32::MAX));
    }

    #[test]
    fn version_parse_accepts_cargo_shapes() {
        let v = Version::parse("0.1.0").unwrap();
        assert_eq!((v.major, v.minor, v.patch, v.pre), (0, 1, 0, None));
        let v = Version::parse("1.2.3-rc.1").unwrap();
        assert_eq!(v.pre, Some("rc.1"));
        assert_eq!(Version::parse("10.20.30").unwrap().patch, 30);
    }

    #[test]
    fn version_parse_rejects_everything_else() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.3-",
            "1.2.3+build",
            "1.2.3-rc/1",
            " 1.2.3",
            "1.2.3 ",
            "1.2.3\n",
            "v1.2.3",
            "1.2.x",
            "-1.2.3",
            "1..3",
            "1.2.3-é",
            "4294967296.0.0",
            "9999999999.0.0",
            "../../etc/passwd",
            "1.2.3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(Version::parse(bad).is_none(), "accepted {bad:?}");
            assert!(!is_valid_version(bad));
        }
    }

    #[test]
    fn version_order_is_numeric_and_prerelease_sorts_first() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.2.0").is_newer_than(&v("0.1.9")));
        assert!(v("0.10.0").is_newer_than(&v("0.9.0")));
        assert!(v("1.0.0").is_newer_than(&v("0.99.99")));
        assert!(v("1.0.0").is_newer_than(&v("1.0.0-rc1")));
        assert!(v("1.0.0-rc2").is_newer_than(&v("1.0.0-rc1")));
        assert!(!v("1.0.0").is_newer_than(&v("1.0.0")));
        assert!(!v("0.1.0").is_newer_than(&v("0.2.0")));
        assert_eq!(v("1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3").cmp(&v("1.2.3")), core::cmp::Ordering::Equal);
    }
}
