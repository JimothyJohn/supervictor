//! DNS hijack server — responds to all queries with the gateway IP.
//!
//! Pure functions compile on all targets.
//! The async `dns_hijack` task is gated behind `#[cfg(feature = "portal")]`.

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

#[cfg(feature = "portal")]
#[embassy_executor::task]
pub async fn dns_hijack(stack: embassy_net::Stack<'static>) {
    use embassy_net::udp::{PacketMetadata, UdpSocket};
    use esp_println::println;

    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 512];
    let mut tx_buf = [0u8; 512];

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(53).unwrap();
    println!("DNS hijack server started on port 53");

    let mut query_buf = [0u8; 512];
    loop {
        let (n, remote) = match socket.recv_from(&mut query_buf).await {
            Ok(result) => result,
            Err(_) => continue,
        };

        if let Some(response) = build_dns_response(&query_buf[..n]) {
            let _ = socket.send_to(&response, remote).await;
        }
    }
}

#[cfg(test)]
#[path = "dns_tests.rs"]
mod tests;
