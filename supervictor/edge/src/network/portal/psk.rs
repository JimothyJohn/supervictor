//! The portal's WPA2 passphrase: the last eight hex digits of the SHA-256
//! fingerprint of the device's own certificate. Whoever issued the
//! certificate can print it (`openssl x509 -fingerprint -sha256`), nobody
//! in radio range can guess it, and no second secret has to be managed.

use heapless::{String as HString, Vec as HVec};
use sha2::{Digest, Sha256};

/// Passphrase length: eight hex digits, the WPA2 minimum.
pub const PSK_LEN: usize = 8;
/// Largest certificate this decodes (DER bytes).
const DER_MAX: usize = 2048;

const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";

/// The first certificate in `pem`, as DER. `None` when the markers or the
/// base64 are off, or the certificate is larger than this decodes.
pub fn der_from_pem(pem: &str) -> Option<HVec<u8, DER_MAX>> {
    let start = pem.find(BEGIN)? + BEGIN.len();
    let end = start + pem[start..].find(END)?;
    base64_decode(&pem[start..end])
}

fn sextet(b: u8) -> Option<u8> {
    Some(match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

/// Standard base64 with padding; whitespace anywhere is skipped.
fn base64_decode(text: &str) -> Option<HVec<u8, DER_MAX>> {
    let mut out = HVec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u8;
    let mut padding = 0u8;
    for &b in text.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None; // data after padding
        }
        acc = (acc << 6) | u32::from(sextet(b)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8).ok()?;
            acc &= (1 << bits) - 1;
        }
    }
    (padding <= 2 && (bits < 6)).then_some(out)
}

/// SHA-256 of the certificate's DER encoding.
pub fn fingerprint(der: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(der);
    h.finalize().into()
}

/// The passphrase for a fingerprint: last four bytes, lowercase hex.
pub fn psk_from_fingerprint(fp: &[u8; 32]) -> HString<PSK_LEN> {
    let mut s = HString::new();
    for b in &fp[28..] {
        let _ = s.push(HEX[usize::from(b >> 4)] as char);
        let _ = s.push(HEX[usize::from(b & 0xf)] as char);
    }
    s
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// PEM in, passphrase out.
pub fn psk_from_pem(pem: &str) -> Option<HString<PSK_LEN>> {
    let der = der_from_pem(pem)?;
    Some(psk_from_fingerprint(&fingerprint(&der)))
}
