//! The smallest DHCP server that gets a phone onto the portal's network:
//! DISCOVER → OFFER, REQUEST → ACK (or NAK for an address that is not
//! ours), one address per client MAC from a short table, everything else
//! ignored. Pure: bytes in, bytes out; the UDP task is in `mod.rs`.
//!
//! RFC 2131 fixed fields are used by offset; options are read and written
//! as the TLV list they are. No lease expiry: the portal runs for minutes
//! and the table holds more clients than will ever join it.

use heapless::Vec as HVec;

/// The portal's own address, gateway and DNS server.
pub const SERVER_IP: [u8; 4] = [192, 168, 4, 1];
/// First address handed out; clients get `.2`, `.3`, …
pub const FIRST_CLIENT: u8 = 2;
/// Clients remembered at once.
pub const MAX_CLIENTS: usize = 8;
/// Lease the client is told it has, in seconds.
pub const LEASE_SECS: u32 = 3600;
/// Biggest reply this server builds.
pub const REPLY_MAX: usize = 300;

const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const OPT_SUBNET: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_END: u8 = 255;
const OPT_PAD: u8 = 0;

const DISCOVER: u8 = 1;
const OFFER: u8 = 2;
const REQUEST: u8 = 3;
const ACK: u8 = 5;
const NAK: u8 = 6;

/// Which MAC holds which slot; slot `i` is `FIRST_CLIENT + i`.
#[derive(Debug, Default)]
pub struct Leases {
    macs: HVec<[u8; 6], MAX_CLIENTS>,
}

impl Leases {
    /// A fresh table.
    pub const fn new() -> Leases {
        Leases { macs: HVec::new() }
    }

    /// The address for `mac`, assigning one if the table has room. A full
    /// table recycles its oldest slot: the portal has one client at a time
    /// in practice, and a stale lease beats no lease.
    pub fn address_for(&mut self, mac: [u8; 6]) -> [u8; 4] {
        let slot = match self.macs.iter().position(|m| *m == mac) {
            Some(i) => i,
            None => {
                if self.macs.is_full() {
                    self.macs.remove(0);
                }
                let _ = self.macs.push(mac);
                self.macs.len() - 1
            }
        };
        [
            SERVER_IP[0],
            SERVER_IP[1],
            SERVER_IP[2],
            FIRST_CLIENT + slot as u8,
        ]
    }

    /// Whether `ip` is the address this table gave `mac`.
    pub fn holds(&self, mac: [u8; 6], ip: [u8; 4]) -> bool {
        self.macs.iter().position(|m| *m == mac).is_some_and(|i| {
            ip == [
                SERVER_IP[0],
                SERVER_IP[1],
                SERVER_IP[2],
                FIRST_CLIENT + i as u8,
            ]
        })
    }
}

/// What a client packet asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Request {
    xid: [u8; 4],
    mac: [u8; 6],
    msg_type: u8,
    requested_ip: Option<[u8; 4]>,
    /// Client's current address (`ciaddr`), set when it renews.
    ciaddr: [u8; 4],
}

fn parse(packet: &[u8]) -> Option<Request> {
    if packet.len() < 240 || packet[0] != 1 || packet[1] != 1 || packet[2] != 6 {
        return None;
    }
    if packet[236..240] != MAGIC_COOKIE {
        return None;
    }
    let mut req = Request {
        xid: [packet[4], packet[5], packet[6], packet[7]],
        mac: [
            packet[28], packet[29], packet[30], packet[31], packet[32], packet[33],
        ],
        msg_type: 0,
        requested_ip: None,
        ciaddr: [packet[12], packet[13], packet[14], packet[15]],
    };
    let mut i = 240;
    while i < packet.len() {
        let code = packet[i];
        if code == OPT_END {
            break;
        }
        if code == OPT_PAD {
            i += 1;
            continue;
        }
        let len = usize::from(*packet.get(i + 1)?);
        let value = packet.get(i + 2..i + 2 + len)?;
        match code {
            OPT_MSG_TYPE if len == 1 => req.msg_type = value[0],
            OPT_REQUESTED_IP if len == 4 => {
                req.requested_ip = Some([value[0], value[1], value[2], value[3]])
            }
            _ => {}
        }
        i += 2 + len;
    }
    (req.msg_type != 0).then_some(req)
}

fn push_opt(out: &mut HVec<u8, REPLY_MAX>, code: u8, value: &[u8]) {
    let _ = out.push(code);
    let _ = out.push(value.len() as u8);
    let _ = out.extend_from_slice(value);
}

fn reply(req: &Request, msg_type: u8, yiaddr: [u8; 4]) -> HVec<u8, REPLY_MAX> {
    let mut out: HVec<u8, REPLY_MAX> = HVec::new();
    let _ = out.extend_from_slice(&[2, 1, 6, 0]);
    let _ = out.extend_from_slice(&req.xid);
    let _ = out.extend_from_slice(&[0, 0, 0x80, 0]); // secs, flags: broadcast
    let _ = out.extend_from_slice(&[0; 4]); // ciaddr
    let _ = out.extend_from_slice(&yiaddr);
    let _ = out.extend_from_slice(&SERVER_IP); // siaddr
    let _ = out.extend_from_slice(&[0; 4]); // giaddr
    let _ = out.extend_from_slice(&req.mac);
    let _ = out.extend_from_slice(&[0; 10]); // rest of chaddr
    let _ = out.extend_from_slice(&[0; 192]); // sname + file
    let _ = out.extend_from_slice(&MAGIC_COOKIE);
    push_opt(&mut out, OPT_MSG_TYPE, &[msg_type]);
    push_opt(&mut out, OPT_SERVER_ID, &SERVER_IP);
    if msg_type != NAK {
        push_opt(&mut out, OPT_LEASE, &LEASE_SECS.to_be_bytes());
        push_opt(&mut out, OPT_SUBNET, &[255, 255, 255, 0]);
        push_opt(&mut out, OPT_ROUTER, &SERVER_IP);
        push_opt(&mut out, OPT_DNS, &SERVER_IP);
    }
    let _ = out.push(OPT_END);
    out
}

/// Handle one client packet. `None` means nothing to send: not DHCP, not
/// for us, or a message type this server does not speak.
pub fn handle(packet: &[u8], leases: &mut Leases) -> Option<HVec<u8, REPLY_MAX>> {
    let req = parse(packet)?;
    match req.msg_type {
        DISCOVER => {
            let ip = leases.address_for(req.mac);
            Some(reply(&req, OFFER, ip))
        }
        REQUEST => {
            // SELECTING / INIT-REBOOT name the address in option 50;
            // RENEWING / REBINDING carry it in ciaddr.
            let wanted = req
                .requested_ip
                .or_else(|| (req.ciaddr != [0; 4]).then_some(req.ciaddr));
            let ours = leases.address_for(req.mac);
            match wanted {
                Some(ip) if ip == ours => Some(reply(&req, ACK, ip)),
                Some(_) => Some(reply(&req, NAK, [0; 4])),
                None => Some(reply(&req, ACK, ours)),
            }
        }
        _ => None,
    }
}

/// The message type of a reply, for logs and tests.
pub fn reply_type(packet: &[u8]) -> Option<u8> {
    parse_reply_type(packet)
}

fn parse_reply_type(packet: &[u8]) -> Option<u8> {
    let mut i = 240;
    while i + 2 <= packet.len() {
        let code = packet[i];
        if code == OPT_END {
            return None;
        }
        if code == OPT_PAD {
            i += 1;
            continue;
        }
        let len = usize::from(packet[i + 1]);
        if code == OPT_MSG_TYPE && len == 1 {
            return packet.get(i + 2).copied();
        }
        i += 2 + len;
    }
    None
}
