//! DNS response builder — answers any query with the gateway IP.
//!
//! Pure functions only, compiled and tested on every target. The UDP
//! hijack task that served captive-portal probes (feature `portal`) was
//! dropped 2026-10-27 with the portal; the builder stays for its return.

use heapless::Vec as HVec;

/// Gateway IP that ALL DNS queries resolve to in AP mode.
pub const GATEWAY: [u8; 4] = [192, 168, 4, 1];

/// Find the end of a DNS QNAME starting at `offset`.
/// QNAME is a sequence of length-prefixed labels ending with a 0 byte.
/// Returns the offset of the byte AFTER the terminating 0.
pub fn find_qname_end(data: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        if offset >= data.len() {
            return None;
        }
        let label_len = data[offset] as usize;
        if label_len == 0 {
            return Some(offset + 1);
        }
        // Pointer (compression) — shouldn't appear in queries, but handle it
        if label_len >= 0xC0 {
            return Some(offset + 2);
        }
        offset += 1 + label_len;
    }
}

/// Build a DNS A-record response for any query.
/// Copies the transaction ID and question from the query,
/// appends a single A record pointing to GATEWAY.
pub fn build_dns_response(query: &[u8]) -> Option<HVec<u8, 512>> {
    // Minimum DNS query: 12 byte header + at least 1-byte name + 4 bytes QTYPE/QCLASS
    if query.len() < 17 {
        return None;
    }

    let mut resp = HVec::<u8, 512>::new();

    // Transaction ID (bytes 0-1)
    resp.extend_from_slice(&query[0..2]).ok()?;

    // Flags: standard response, recursion available, no error
    resp.extend_from_slice(&[0x81, 0x80]).ok()?;

    // Question count (bytes 4-5, copied from query)
    resp.extend_from_slice(&query[4..6]).ok()?;

    // Answer count: 1
    resp.extend_from_slice(&[0x00, 0x01]).ok()?;

    // Authority RR count: 0
    resp.extend_from_slice(&[0x00, 0x00]).ok()?;

    // Additional RR count: 0
    resp.extend_from_slice(&[0x00, 0x00]).ok()?;

    // Question section — copy verbatim from query
    let qname_end = find_qname_end(query, 12)?;
    let question_end = qname_end + 4; // +2 QTYPE + 2 QCLASS
    if question_end > query.len() {
        return None;
    }
    resp.extend_from_slice(&query[12..question_end]).ok()?;

    // Answer section: A record pointing to gateway
    resp.extend_from_slice(&[
        0xC0, 0x0C, // Name: pointer to question name at offset 12
        0x00, 0x01, // Type: A
        0x00, 0x01, // Class: IN
        0x00, 0x00, 0x00, 0x3C, // TTL: 60 seconds
        0x00, 0x04, // Data length: 4 (IPv4)
    ])
    .ok()?;
    resp.extend_from_slice(&GATEWAY).ok()?;

    Some(resp)
}
