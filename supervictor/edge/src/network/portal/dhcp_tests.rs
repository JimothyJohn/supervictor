extern crate std;

use std::vec::Vec;

use super::dhcp::*;
use proptest::prelude::*;

const MAC_A: [u8; 6] = [0xd4, 0xf9, 0x8d, 0x04, 0x35, 0xf8];
const MAC_B: [u8; 6] = [0x10, 0x97, 0xbd, 0xe5, 0x9b, 0x0c];

fn client_packet(
    msg_type: u8,
    xid: [u8; 4],
    mac: [u8; 6],
    ciaddr: [u8; 4],
    options: &[(u8, &[u8])],
) -> Vec<u8> {
    let mut p = std::vec![0u8; 240];
    p[0] = 1;
    p[1] = 1;
    p[2] = 6;
    p[4..8].copy_from_slice(&xid);
    p[12..16].copy_from_slice(&ciaddr);
    p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    p.extend_from_slice(&[53, 1, msg_type]);
    for (code, value) in options {
        p.push(*code);
        p.push(value.len() as u8);
        p.extend_from_slice(value);
    }
    p.push(255);
    p
}

fn option(reply: &[u8], code: u8) -> Option<&[u8]> {
    let mut i = 240;
    while i + 2 <= reply.len() {
        let c = reply[i];
        if c == 255 {
            return None;
        }
        if c == 0 {
            i += 1;
            continue;
        }
        let len = usize::from(reply[i + 1]);
        if c == code {
            return Some(&reply[i + 2..i + 2 + len]);
        }
        i += 2 + len;
    }
    None
}

#[test]
fn discover_gets_an_offer_with_the_portal_as_everything() {
    let mut leases = Leases::new();
    let reply = handle(
        &client_packet(1, [1, 2, 3, 4], MAC_A, [0; 4], &[]),
        &mut leases,
    )
    .unwrap();
    assert_eq!(reply[0], 2, "BOOTREPLY");
    assert_eq!(&reply[4..8], &[1, 2, 3, 4], "xid echoed");
    assert_eq!(&reply[16..20], &[192, 168, 4, 2], "yiaddr");
    assert_eq!(&reply[20..24], &SERVER_IP, "siaddr");
    assert_eq!(&reply[28..34], &MAC_A, "chaddr echoed");
    assert_eq!(&reply[236..240], &[99, 130, 83, 99]);
    assert_eq!(reply_type(&reply), Some(2), "OFFER");
    assert_eq!(option(&reply, 54), Some(&SERVER_IP[..]), "server id");
    assert_eq!(option(&reply, 1), Some(&[255, 255, 255, 0][..]), "mask");
    assert_eq!(option(&reply, 3), Some(&SERVER_IP[..]), "router");
    assert_eq!(option(&reply, 6), Some(&SERVER_IP[..]), "dns");
    assert_eq!(option(&reply, 51), Some(&LEASE_SECS.to_be_bytes()[..]));
    assert!(reply.len() <= REPLY_MAX);
}

#[test]
fn request_for_the_offered_address_is_acked() {
    let mut leases = Leases::new();
    let _ = handle(&client_packet(1, [1; 4], MAC_A, [0; 4], &[]), &mut leases).unwrap();
    let reply = handle(
        &client_packet(3, [1; 4], MAC_A, [0; 4], &[(50, &[192, 168, 4, 2])]),
        &mut leases,
    )
    .unwrap();
    assert_eq!(reply_type(&reply), Some(5), "ACK");
    assert_eq!(&reply[16..20], &[192, 168, 4, 2]);
}

#[test]
fn request_for_someone_elses_address_is_nakked() {
    let mut leases = Leases::new();
    let reply = handle(
        &client_packet(3, [1; 4], MAC_A, [0; 4], &[(50, &[10, 0, 0, 7])]),
        &mut leases,
    )
    .unwrap();
    assert_eq!(reply_type(&reply), Some(6), "NAK");
    assert_eq!(&reply[16..20], &[0, 0, 0, 0]);
    assert!(option(&reply, 51).is_none(), "no lease in a NAK");
}

#[test]
fn renew_by_ciaddr_is_acked_and_a_bare_request_gets_its_address() {
    let mut leases = Leases::new();
    let _ = handle(&client_packet(1, [1; 4], MAC_B, [0; 4], &[]), &mut leases).unwrap();
    let reply = handle(
        &client_packet(3, [2; 4], MAC_B, [192, 168, 4, 2], &[]),
        &mut leases,
    )
    .unwrap();
    assert_eq!(reply_type(&reply), Some(5));
    let reply = handle(&client_packet(3, [3; 4], MAC_B, [0; 4], &[]), &mut leases).unwrap();
    assert_eq!(reply_type(&reply), Some(5));
    assert_eq!(&reply[16..20], &[192, 168, 4, 2]);
}

#[test]
fn clients_keep_their_address_and_distinct_clients_differ() {
    let mut leases = Leases::new();
    assert_eq!(leases.address_for(MAC_A), [192, 168, 4, 2]);
    assert_eq!(leases.address_for(MAC_B), [192, 168, 4, 3]);
    assert_eq!(leases.address_for(MAC_A), [192, 168, 4, 2]);
    assert!(leases.holds(MAC_A, [192, 168, 4, 2]));
    assert!(!leases.holds(MAC_A, [192, 168, 4, 3]));
    assert!(!leases.holds([0; 6], [192, 168, 4, 2]));
}

#[test]
fn a_full_table_recycles_and_never_hands_out_the_server_address() {
    let mut leases = Leases::new();
    for i in 0..(MAX_CLIENTS as u8 + 3) {
        let ip = leases.address_for([i; 6]);
        assert_ne!(ip, SERVER_IP);
        assert!(
            ip[3] >= FIRST_CLIENT && ip[3] < FIRST_CLIENT + MAX_CLIENTS as u8,
            "{ip:?}"
        );
    }
}

#[test]
fn other_message_types_and_non_dhcp_are_ignored() {
    let mut leases = Leases::new();
    for t in [2u8, 4, 5, 6, 7, 8] {
        assert!(
            handle(&client_packet(t, [1; 4], MAC_A, [0; 4], &[]), &mut leases).is_none(),
            "type {t}"
        );
    }
    let mut p = client_packet(1, [1; 4], MAC_A, [0; 4], &[]);
    p[0] = 2; // a reply, not a request
    assert!(handle(&p, &mut leases).is_none());
    let mut p = client_packet(1, [1; 4], MAC_A, [0; 4], &[]);
    p[236] = 0; // no magic cookie
    assert!(handle(&p, &mut leases).is_none());
    assert!(handle(&[0u8; 100], &mut leases).is_none());
    assert!(handle(&[], &mut leases).is_none());
    // Message type option missing entirely.
    let mut p = client_packet(1, [1; 4], MAC_A, [0; 4], &[]);
    p.truncate(240);
    p.push(255);
    assert!(handle(&p, &mut leases).is_none());
}

#[test]
fn padding_and_unknown_options_are_skipped() {
    let mut leases = Leases::new();
    let mut p = client_packet(
        1,
        [1; 4],
        MAC_A,
        [0; 4],
        &[(61, &[1, 2, 3, 4, 5, 6, 7]), (55, &[1, 3, 6])],
    );
    // Sprinkle pads before END.
    let end = p.pop().unwrap();
    p.extend_from_slice(&[0, 0, 0]);
    p.push(end);
    assert_eq!(
        handle(&p, &mut leases).map(|r| reply_type(&r)),
        Some(Some(2))
    );
}

#[test]
fn a_truncated_option_list_is_not_a_request() {
    let mut leases = Leases::new();
    let mut p = client_packet(1, [1; 4], MAC_A, [0; 4], &[]);
    p.truncate(240);
    p.extend_from_slice(&[50, 4, 192, 168]); // option claims 4 bytes, has 2
    assert!(handle(&p, &mut leases).is_none());
}

proptest! {
    #[test]
    fn any_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut leases = Leases::new();
        let _ = handle(&bytes, &mut leases);
        let _ = reply_type(&bytes);
    }

    #[test]
    fn any_discover_yields_a_well_formed_offer(
        xid in any::<[u8; 4]>(), mac in any::<[u8; 6]>(),
        extra in proptest::collection::vec((1u8..=250, proptest::collection::vec(any::<u8>(), 0..8)), 0..4)
    ) {
        let mut leases = Leases::new();
        let opts: Vec<(u8, &[u8])> = extra.iter().filter(|(c, _)| *c != 53 && *c != 50).map(|(c, v)| (*c, v.as_slice())).collect();
        let p = client_packet(1, xid, mac, [0; 4], &opts);
        let r = handle(&p, &mut leases).unwrap();
        prop_assert_eq!(reply_type(&r), Some(2));
        prop_assert_eq!(&r[4..8], &xid);
        prop_assert_eq!(&r[28..34], &mac);
        prop_assert!(r.len() <= REPLY_MAX);
    }
}
