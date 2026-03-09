use super::*;

// Minimal valid DNS A query for "example.com"
// 12 header + 13 QNAME + 4 QTYPE/QCLASS = 29 bytes
const EXAMPLE_COM_QUERY: [u8; 29] = [
    0xAB, 0xCD, // Transaction ID
    0x01, 0x00, // Flags: standard query
    0x00, 0x01, // Questions: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Answers, Authority, Additional: 0
    // QNAME: example.com
    0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm',
    0x00, // End of QNAME
    0x00, 0x01, // QTYPE: A
    0x00, 0x01, // QCLASS: IN
];

// --- find_qname_end ---

#[test]
fn find_qname_end_single_label() {
    let data = [0x05, b'h', b'e', b'l', b'l', b'o', 0x00];
    assert_eq!(find_qname_end(&data, 0), Some(7));
}

#[test]
fn find_qname_end_multi_label() {
    let data = [
        0x03, b'w', b'w', b'w', 0x06, b'g', b'o', b'o', b'g', b'l', b'e', 0x03, b'c', b'o', b'm',
        0x00,
    ];
    assert_eq!(find_qname_end(&data, 0), Some(16));
}

#[test]
fn find_qname_end_with_offset() {
    // QNAME "example.com" starts at offset 12 in the query
    // 7+example(7) + 3+com(3) + 0 = 13 bytes → ends at 12+13=25
    assert_eq!(find_qname_end(&EXAMPLE_COM_QUERY, 12), Some(25));
}

#[test]
fn find_qname_end_empty_buffer() {
    assert_eq!(find_qname_end(&[], 0), None);
}

#[test]
fn find_qname_end_truncated() {
    let data = [0x05, b'h', b'e']; // Label says 5 bytes but only 2 follow
    assert_eq!(find_qname_end(&data, 0), None);
}

#[test]
fn find_qname_end_root() {
    let data = [0x00]; // Root name: just terminator
    assert_eq!(find_qname_end(&data, 0), Some(1));
}

#[test]
fn find_qname_end_pointer() {
    let data = [0xC0, 0x0C]; // Compressed name pointer
    assert_eq!(find_qname_end(&data, 0), Some(2));
}

// --- build_dns_response ---

#[test]
fn build_dns_response_valid_query() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    assert!(resp.len() > 12);
}

#[test]
fn build_dns_response_preserves_txn_id() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    assert_eq!(resp[0], 0xAB);
    assert_eq!(resp[1], 0xCD);
}

#[test]
fn build_dns_response_flags() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    assert_eq!(resp[2], 0x81);
    assert_eq!(resp[3], 0x80);
}

#[test]
fn build_dns_response_answer_count() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    assert_eq!(resp[6], 0x00);
    assert_eq!(resp[7], 0x01);
}

#[test]
fn build_dns_response_gateway_ip() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    let ip = &resp[resp.len() - 4..];
    assert_eq!(ip, &[192, 168, 4, 1]);
}

#[test]
fn build_dns_response_a_record_type() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    let qname_end = find_qname_end(&EXAMPLE_COM_QUERY, 12).unwrap();
    let question_end = qname_end + 4;
    let answer_start = 12 + (question_end - 12);
    // Name pointer
    assert_eq!(resp[answer_start], 0xC0);
    assert_eq!(resp[answer_start + 1], 0x0C);
    // Type A
    assert_eq!(resp[answer_start + 2], 0x00);
    assert_eq!(resp[answer_start + 3], 0x01);
}

#[test]
fn build_dns_response_ttl_60() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    let qname_end = find_qname_end(&EXAMPLE_COM_QUERY, 12).unwrap();
    let question_end = qname_end + 4;
    let answer_start = 12 + (question_end - 12);
    // Name(2) + Type(2) + Class(2) = 6, then TTL(4)
    let ttl_offset = answer_start + 6;
    assert_eq!(resp[ttl_offset], 0x00);
    assert_eq!(resp[ttl_offset + 1], 0x00);
    assert_eq!(resp[ttl_offset + 2], 0x00);
    assert_eq!(resp[ttl_offset + 3], 0x3C); // 60 seconds
}

#[test]
fn build_dns_response_too_short() {
    assert!(build_dns_response(&[0u8; 16]).is_none());
}

#[test]
fn build_dns_response_question_copied() {
    let resp = build_dns_response(&EXAMPLE_COM_QUERY).unwrap();
    let qname_end = find_qname_end(&EXAMPLE_COM_QUERY, 12).unwrap();
    let question_end = qname_end + 4;
    let question_bytes = &EXAMPLE_COM_QUERY[12..question_end];
    let resp_question = &resp[12..12 + question_bytes.len()];
    assert_eq!(resp_question, question_bytes);
}

// Android captive portal probe: connectivitycheck.gstatic.com
// 12 header + 18(connectivitycheck) + 8(gstatic) + 4(com) + 1(null) + 4(QTYPE/QCLASS) = 47
const ANDROID_PROBE: [u8; 47] = [
    0x12, 0x34, // Transaction ID
    0x01, 0x00, // Flags
    0x00, 0x01, // Questions: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // QNAME: connectivitycheck.gstatic.com
    17, b'c', b'o', b'n', b'n', b'e', b'c', b't', b'i', b'v', b'i', b't', b'y', b'c', b'h', b'e',
    b'c', b'k', 7, b'g', b's', b't', b'a', b't', b'i', b'c', 3, b'c', b'o', b'm',
    0x00, // End of QNAME
    0x00, 0x01, // QTYPE: A
    0x00, 0x01, // QCLASS: IN
];

#[test]
fn build_dns_response_android_captive_portal() {
    let resp = build_dns_response(&ANDROID_PROBE).unwrap();
    assert_eq!(resp[0], 0x12);
    assert_eq!(resp[1], 0x34);
    assert_eq!(resp[2], 0x81);
    assert_eq!(resp[3], 0x80);
    let ip = &resp[resp.len() - 4..];
    assert_eq!(ip, GATEWAY);
}

// iOS captive portal probe: captive.apple.com
// 12 header + 8(captive) + 6(apple) + 4(com) + 1(null) + 4(QTYPE/QCLASS) = 35
const IOS_PROBE: [u8; 35] = [
    0x56, 0x78, // Transaction ID
    0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // QNAME: captive.apple.com
    7, b'c', b'a', b'p', b't', b'i', b'v', b'e', 5, b'a', b'p', b'p', b'l', b'e', 3, b'c', b'o',
    b'm', 0x00, // End of QNAME
    0x00, 0x01, // QTYPE: A
    0x00, 0x01, // QCLASS: IN
];

#[test]
fn build_dns_response_ios_captive_portal() {
    let resp = build_dns_response(&IOS_PROBE).unwrap();
    assert_eq!(resp[0], 0x56);
    assert_eq!(resp[1], 0x78);
    let ip = &resp[resp.len() - 4..];
    assert_eq!(ip, GATEWAY);
}

#[test]
fn build_dns_response_different_txn_ids() {
    // Two queries with different transaction IDs produce different responses
    let mut q1 = EXAMPLE_COM_QUERY;
    let mut q2 = EXAMPLE_COM_QUERY;
    q1[0] = 0x00;
    q1[1] = 0x01;
    q2[0] = 0xFF;
    q2[1] = 0xFE;

    let r1 = build_dns_response(&q1).unwrap();
    let r2 = build_dns_response(&q2).unwrap();

    assert_eq!(r1[0], 0x00);
    assert_eq!(r1[1], 0x01);
    assert_eq!(r2[0], 0xFF);
    assert_eq!(r2[1], 0xFE);
}
