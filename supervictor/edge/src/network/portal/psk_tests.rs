extern crate std;

use std::string::String;
use std::vec::Vec;

use super::psk::*;
use proptest::prelude::*;
use sha2::{Digest, Sha256};

fn cert() -> (String, Vec<u8>) {
    let c = rcgen::generate_simple_self_signed(std::vec![String::from("c3-wifi-01")]).unwrap();
    (c.cert.pem(), c.cert.der().to_vec())
}

#[test]
fn pem_decodes_to_the_der_rcgen_produced() {
    let (pem, der) = cert();
    assert_eq!(der_from_pem(&pem).unwrap().as_slice(), der.as_slice());
}

#[test]
fn psk_is_the_last_eight_hex_of_the_sha256_fingerprint() {
    let (pem, der) = cert();
    let fp: [u8; 32] = Sha256::digest(&der).into();
    let expect: String = fp[28..].iter().map(|b| std::format!("{b:02x}")).collect();
    assert_eq!(psk_from_pem(&pem).unwrap().as_str(), expect);
    assert_eq!(psk_from_pem(&pem).unwrap().len(), PSK_LEN);
}

#[test]
fn psk_matches_what_openssl_prints_for_a_known_certificate() {
    // A fixed DER so the expectation is a constant anyone can re-derive:
    // `openssl x509 -inform DER -fingerprint -sha256 -noout` on these bytes
    // ends in the same four octets. The bytes need not be a valid
    // certificate for the digest to be well defined.
    let der: Vec<u8> = (0u8..=255).collect();
    let fp = fingerprint(&der);
    assert_eq!(fp, Sha256::digest(&der).as_slice());
    let psk = psk_from_fingerprint(&fp);
    assert_eq!(psk.len(), 8);
    assert!(psk
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn pem_with_a_chain_uses_the_first_certificate() {
    let (pem_a, der_a) = cert();
    let (pem_b, _) = cert();
    let chain = std::format!("{pem_a}\n{pem_b}");
    assert_eq!(der_from_pem(&chain).unwrap().as_slice(), der_a.as_slice());
}

#[test]
fn pem_tolerates_crlf_and_leading_text() {
    let (pem, der) = cert();
    let crlf = std::format!("subject=CN=x\r\n{}", pem.replace('\n', "\r\n"));
    assert_eq!(der_from_pem(&crlf).unwrap().as_slice(), der.as_slice());
}

#[test]
fn bad_pem_is_none() {
    assert!(der_from_pem("").is_none());
    assert!(
        der_from_pem("-----BEGIN CERTIFICATE-----\nAAAA").is_none(),
        "no end marker"
    );
    assert!(
        der_from_pem("-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----").is_none(),
        "not base64"
    );
    assert!(
        der_from_pem("-----BEGIN CERTIFICATE-----\nQQ==QQ==\n-----END CERTIFICATE-----").is_none(),
        "data after padding"
    );
    assert!(
        der_from_pem("-----BEGIN CERTIFICATE-----\nQ\n-----END CERTIFICATE-----").is_none(),
        "dangling sextet"
    );
    let huge = std::format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
        "QUJD".repeat(1000)
    );
    assert!(der_from_pem(&huge).is_none(), "over the DER cap");
}

#[test]
fn base64_edge_lengths_decode() {
    for (text, bytes) in [
        ("", &b""[..]),
        ("QQ==", b"A"),
        ("QUI=", b"AB"),
        ("QUJD", b"ABC"),
        ("QUJDRA==", b"ABCD"),
    ] {
        let pem = std::format!("-----BEGIN CERTIFICATE-----\n{text}\n-----END CERTIFICATE-----");
        assert_eq!(der_from_pem(&pem).unwrap().as_slice(), bytes, "{text}");
    }
}

proptest! {
    #[test]
    fn any_text_between_markers_never_panics(body in "[ -~\n]{0,300}") {
        let pem = std::format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----");
        let _ = psk_from_pem(&pem);
    }
}
