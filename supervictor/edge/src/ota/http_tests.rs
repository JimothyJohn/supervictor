extern crate std;

use super::http::*;
use proptest::prelude::*;
use std::format;

#[test]
fn manifest_request_shape() {
    let r = manifest_request(
        "staging.supervictor.advin.io",
        "/",
        "poe-iso-rack-02",
        "0.2.0",
    )
    .unwrap();
    assert!(
        r.starts_with("GET /devices/poe-iso-rack-02/firmware/0.2.0/manifest HTTP/1.1\r\n"),
        "{r}"
    );
    assert!(r.contains("Host: staging.supervictor.advin.io\r\n"));
    assert!(r.contains("Connection: close\r\n"));
    assert!(r.ends_with("\r\n\r\n"));
    // A stage prefix (raw execute-api) is folded in without a double slash.
    let r = manifest_request("h", "/staging/", "d", "1.0.0").unwrap();
    assert!(
        r.starts_with("GET /staging/devices/d/firmware/1.0.0/manifest "),
        "{r}"
    );
    let r = manifest_request("h", "/staging", "d", "1.0.0").unwrap();
    assert!(
        r.starts_with("GET /staging/devices/d/firmware/1.0.0/manifest "),
        "{r}"
    );
}

#[test]
fn image_request_carries_range_and_if_match() {
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let r = image_request("h", "/", "d", "0.2.0", 0, sha).unwrap();
    assert!(
        r.starts_with("GET /devices/d/firmware/0.2.0 HTTP/1.1\r\n"),
        "{r}"
    );
    assert!(r.contains("Range: bytes=0-\r\n"), "{r}");
    assert!(r.contains(&format!("If-Match: \"{sha}\"\r\n")), "{r}");
    let r = image_request("h", "/", "d", "0.2.0", 1_048_576, sha).unwrap();
    assert!(r.contains("Range: bytes=1048576-\r\n"), "{r}");
}

#[test]
fn requests_with_the_longest_real_identifiers_fit() {
    let host = "staging.supervictor.advin.io";
    let id = "x".repeat(64);
    let version = "100.200.300-release.candidate.1";
    assert!(manifest_request(host, "/staging", &id, version).is_ok());
    let sha = "f".repeat(64);
    assert!(image_request(host, "/staging", &id, version, u32::MAX, &sha).is_ok());
    // And an absurd one overflows as an error, not a truncated request.
    let huge = "y".repeat(400);
    assert!(matches!(
        manifest_request(host, "/", &huge, version),
        Err(crate::error::HttpError::BufferOverflow)
    ));
}

const HEAD_206: &[u8] = b"HTTP/1.1 206 Partial Content\r\ncontent-type: application/octet-stream\r\naccept-ranges: bytes\r\netag: \"abc\"\r\ncontent-length: 4096\r\ncontent-range: bytes 8192-12287/1085760\r\ndate: Sat, 25 Oct 2026 21:00:00 GMT\r\n\r\nBODY";

#[test]
fn parses_a_partial_content_head_and_reports_the_body_offset() {
    let (head, n) = parse_head(HEAD_206).unwrap();
    assert_eq!(head.status, 206);
    assert_eq!(head.content_length, Some(4096));
    assert_eq!(head.content_range, Some((8192, 1_085_760)));
    assert_eq!(&HEAD_206[n..], b"BODY");
}

#[test]
fn incomplete_heads_ask_for_more() {
    for cut in 0..HEAD_206.len() - 4 {
        if HEAD_206[..cut].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        assert_eq!(
            parse_head(&HEAD_206[..cut]),
            Err(HeadError::Incomplete),
            "cut {cut}"
        );
    }
}

#[test]
fn header_names_are_case_insensitive_and_unknown_ones_skipped() {
    let buf = b"HTTP/1.1 200 OK\r\nX-Whatever: yes\r\nCONTENT-LENGTH: 10\r\nContent-Range:bytes 0-9/10\r\n\r\n0123456789";
    let (head, n) = parse_head(buf).unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.content_length, Some(10));
    assert_eq!(head.content_range, Some((0, 10)));
    assert_eq!(buf.len() - n, 10);
}

#[test]
fn rejects_what_must_not_reach_flash() {
    // Two Content-Lengths that disagree: request smuggling territory.
    assert_eq!(
        parse_head(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Length: 20\r\n\r\n"),
        Err(HeadError::Unsupported)
    );
    // Agreeing duplicates are tolerated.
    assert!(
        parse_head(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Length: 10\r\n\r\n").is_ok()
    );
    // Chunked: not implemented, refused loudly.
    assert_eq!(
        parse_head(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"),
        Err(HeadError::Unsupported)
    );
    // Larger than a slot.
    assert_eq!(
        parse_head(b"HTTP/1.1 200 OK\r\nContent-Length: 2031617\r\n\r\n"),
        Err(HeadError::TooLarge)
    );
    assert_eq!(
        parse_head(b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999\r\n\r\n"),
        Err(HeadError::Malformed)
    );
    assert_eq!(
        parse_head(b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-9/2031617\r\n\r\n"),
        Err(HeadError::TooLarge)
    );
    // Malformed shapes.
    for bad in [
        &b"HTTP/2 200 OK\r\n\r\n"[..],
        b"HTTP/1.1 20 OK\r\n\r\n",
        b"HTTP/1.1 2000 OK\r\n\r\n",
        b"HTTP/1.1 abc OK\r\n\r\n",
        b"200 OK\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nno colon here\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: -1\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 1 0\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: \r\n\r\n",
        b"HTTP/1.1 206 OK\r\nContent-Range: bytes 10-9/100\r\n\r\n",
        b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-100/100\r\n\r\n",
        b"HTTP/1.1 206 OK\r\nContent-Range: items 0-9/100\r\n\r\n",
        b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-9\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\xff\r\n\r\n",
    ] {
        assert_eq!(
            parse_head(bad),
            Err(HeadError::Malformed),
            "{:?}",
            core::str::from_utf8(bad)
        );
    }
}

#[test]
fn unsatisfiable_range_reports_nothing_left() {
    let (head, _) = parse_head(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */1000\r\nContent-Length: 0\r\n\r\n").unwrap();
    assert_eq!(head.status, 416);
    assert_eq!(head.content_range, Some((1000, 1000)));
}

#[test]
fn head_without_body_hints_is_fine() {
    let (head, n) = parse_head(b"HTTP/1.1 412 Precondition Failed\r\n\r\n").unwrap();
    assert_eq!(head.status, 412);
    assert_eq!(head.content_length, None);
    assert_eq!(n, 36);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(3000))]

    /// Arbitrary bytes never panic the parser, and a success always points
    /// inside the buffer.
    #[test]
    fn parse_head_never_panics(buf in prop::collection::vec(any::<u8>(), 0..300)) {
        if let Ok((_, n)) = parse_head(&buf) {
            prop_assert!(n <= buf.len());
            prop_assert!(n >= 4);
        }
    }

    /// Splicing a valid head with random header lines either parses to the
    /// same status or fails cleanly — never a different status.
    #[test]
    fn extra_headers_cannot_change_the_status(junk in "[ -~]{0,60}") {
        let mut buf = b"HTTP/1.1 206 Partial Content\r\n".to_vec();
        buf.extend_from_slice(junk.as_bytes());
        buf.extend_from_slice(b"\r\nContent-Length: 5\r\n\r\nhello");
        match parse_head(&buf) {
            Ok((head, _)) => prop_assert_eq!(head.status, 206),
            Err(e) => prop_assert_ne!(e, HeadError::Incomplete),
        }
    }
}
