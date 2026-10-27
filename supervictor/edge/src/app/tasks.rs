use crate::config::*;
#[cfg(feature = "sim")]
use crate::models::profile::Profile;
use crate::models::rng::seed_from;
#[cfg(not(feature = "sim"))]
use crate::models::rng::{jitter_ms, Xorshift64};
#[cfg(feature = "sim")]
use crate::models::sim::{Hardware, Simulator};
use crate::models::uplink::UplinkAck;
#[cfg(not(feature = "sim"))]
use crate::models::uplink::UplinkMessage;
use crate::network::http::post_request;
use crate::network::tls::load_certificates;
use crate::ota::device::Ota;
use crate::ota::http::{parse_head, HeadError};
use embassy_net::tcp::TcpSocket;
#[cfg(feature = "wifi")]
use embassy_net::Runner;
use embassy_net::Stack;
use embassy_time::{Duration, Instant, Timer};
#[cfg(feature = "embedded")]
use esp_hal::rtc_cntl::Rwdt;
use esp_println::println;
#[cfg(feature = "wifi")]
use esp_radio::wifi::{WifiController, WifiDevice, WifiEvent, WifiStaState};
use mbedtls_rs::{Session, SessionConfig, Tls};

/// Uplink path from API_PATH at compile time: "/" for the endpoint itself,
/// a prefix when something in front mounts it under one. Must start with
/// '/'. The OTA routes hang off the same prefix.
pub const API_PATH: &str = env!("API_PATH");
const _: () = assert!(!API_PATH.is_empty() && API_PATH.as_bytes()[0] == b'/');

/// After a failed update attempt, leave the version alone this long
/// before trying again on a later uplink — the uplink loop must not turn
/// into a download loop against a broken image or a flaky link.
const OTA_RETRY_DELAY: Duration = Duration::from_secs(120);

/// How an uplink response read ended.
enum Ack {
    /// A framed response: status and the body bytes that arrived.
    Framed {
        status: u16,
        body: heapless::Vec<u8, 1024>,
    },
    /// Peer closed before any head.
    Closed,
    /// Read error or timeout; the connection is unusable.
    Broken,
}

/// Read the uplink response: head, then body to `Content-Length` (or EOF
/// / buffer full). Any framed head is a completed round-trip; the body is
/// only inspected on a 2xx.
async fn read_ack<T>(session: &mut Session<'_, T>) -> Ack
where
    T: embedded_io_async::Read + embedded_io_async::Write,
{
    let mut buf = [0u8; 1024];
    let mut filled = 0usize;
    let head = loop {
        match parse_head(&buf[..filled]) {
            Ok(h) => break h,
            Err(HeadError::Incomplete) if filled < buf.len() => {}
            Err(_) => return Ack::Broken,
        }
        match embassy_time::with_timeout(HTTP_READ_TIMEOUT, session.read(&mut buf[filled..])).await
        {
            Ok(Ok(0)) => {
                return if filled == 0 {
                    Ack::Closed
                } else {
                    Ack::Broken
                }
            }
            Ok(Ok(n)) => filled += n,
            Ok(Err(e)) => {
                println!("   Read failed: {:?}", e);
                return Ack::Broken;
            }
            Err(_) => {
                println!("   Read timed out");
                return Ack::Broken;
            }
        }
    };
    let (head, body_start) = head;
    let want = head.content_length.map(|n| n as usize);
    while want.is_some_and(|w| filled - body_start < w) && filled < buf.len() {
        match embassy_time::with_timeout(HTTP_READ_TIMEOUT, session.read(&mut buf[filled..])).await
        {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => filled += n,
            _ => break,
        }
    }
    let end = match want {
        Some(w) => (body_start + w).min(filled),
        None => filled,
    };
    let mut body = heapless::Vec::new();
    let _ = body.extend_from_slice(&buf[body_start..end]);
    Ack::Framed {
        status: head.status,
        body,
    }
}

async fn backoff_delay(backoff: &mut Duration) {
    println!("   retrying in {}ms", backoff.as_millis());
    Timer::after(*backoff).await;
    let doubled_ms = backoff.as_millis().saturating_mul(2);
    *backoff = if doubled_ms > BACKOFF_MAX.as_millis() {
        BACKOFF_MAX
    } else {
        Duration::from_millis(doubled_ms)
    };
}

/// Maintains the WiFi station connection, reconnecting on disconnect.
#[cfg(feature = "wifi")]
#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    loop {
        if esp_radio::wifi::sta_state() == WifiStaState::Connected {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            println!("WiFi disconnected, reconnecting...");
            Timer::after(Duration::from_millis(5000)).await;
        }

        if !matches!(controller.is_started(), Ok(true)) {
            if let Err(e) = controller.start_async().await {
                println!("WiFi start_async failed: {e:?}");
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
        }

        match controller.connect_async().await {
            Ok(_) => println!("Connected to WiFi!"),
            Err(e) => {
                println!("Failed to connect to WiFi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await;
            }
        }
    }
}

/// Drives the Embassy network stack (packet rx/tx) for the WiFi interface.
#[cfg(feature = "wifi")]
#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

/// Device identity on the wire: `DEVICE_NAME` at build time (what the
/// Quickstart flashes with), falling back to the historical bench id.
pub const DEVICE_ID_ON_WIRE: &str = match option_env!("DEVICE_NAME") {
    Some(name) => name,
    None => "1234567890",
};

/// Machine the telemetry simulator impersonates: `SIM_PROFILE` at build time
/// (see `models::profile::Profile::parse` for the names), default the bench
/// trace. An unknown name is a compile error, not a silent fallback.
#[cfg(feature = "sim")]
pub const SIM_PROFILE: Profile = match option_env!("SIM_PROFILE") {
    None => Profile::Generic,
    Some(name) => match Profile::parse(name) {
        Some(profile) => profile,
        None => panic!("SIM_PROFILE is not a known machine profile"),
    },
};

/// Simulated seconds per real second for the machine model (`SIM_SPEED` at
/// build time, default 1): a compressed working day for demos.
#[cfg(feature = "sim")]
pub const SIM_SPEED: u32 = match option_env!("SIM_SPEED") {
    None => 1,
    Some(s) => parse_u32(s),
};

/// Decimal `u32` parser usable in a `const`; rejects anything else at build time.
#[cfg(feature = "sim")]
const fn parse_u32(s: &str) -> u32 {
    let bytes = s.as_bytes();
    assert!(!bytes.is_empty(), "SIM_SPEED must be a positive integer");
    let mut n: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let d = bytes[i];
        assert!(d.is_ascii_digit(), "SIM_SPEED must be a positive integer");
        n = n * 10 + (d - b'0') as u32;
        i += 1;
    }
    n
}

/// Main application loop: resolves DNS, establishes mTLS, and sends uplink
/// telemetry. `seed` (hardware RNG) salts the telemetry simulator so two
/// boots of the same board don't replay the same trace. `ota` is `None`
/// on a board still on the factory partition table. The TCP buffers are
/// statics: the task future is built on the boot stack before it moves
/// into its pool, and 8 KB of buffers (plus OTA's) overflowed it.
#[cfg(feature = "embedded")]
#[embassy_executor::task]
pub async fn app(
    stack: Stack<'static>,
    tls: Tls<'static>,
    mut rwdt: Rwdt,
    seed: u64,
    mut ota: Option<&'static mut Ota>,
    rx_buffer: &'static mut [u8; TCP_RX_BUFFER_SIZE],
    tx_buffer: &'static mut [u8; TCP_TX_BUFFER_SIZE],
) {
    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
        if stack.is_link_up() {
            break;
        }
        println!("Initializing network stack...");
    }

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
        if let Some(_config) = stack.config_v4() {
            break;
        }
        println!("Waiting to get IP address...");
    }

    // Device identity comes from DEVICE_NAME at compile time — set per board
    // at flash time (the onboard flow does this); .cargo/config.toml provides
    // the "factory-01" default. Length-checked at compile time against the
    // UplinkMessage id capacity so a long name can't panic on boot.
    const _: () = assert!(DEVICE_ID_ON_WIRE.len() <= 64);
    let device_id: heapless::String<64> = DEVICE_ID_ON_WIRE.try_into().unwrap_or_default();

    // Feature `sim`: simulated readings until real sensors land
    // (docs/TELEMETRY.md): a per-board personality from the device id,
    // salted by the hardware RNG so two boots don't replay the same trace.
    // Heap and uptime stay real. SIM_PROFILE makes the board act like a
    // machine (models/profile.rs).
    #[cfg(feature = "sim")]
    let mut sim = Simulator::with_profile(seed_from(device_id.as_str(), seed), SIM_PROFILE);
    #[cfg(feature = "sim")]
    {
        sim.set_time_scale(SIM_SPEED);
        println!(
            "Telemetry profile: {} (x{} time)",
            SIM_PROFILE.name(),
            SIM_SPEED
        );
    }
    // Without it the board reports only what it measured. No sensor driver
    // exists yet (HARDWARE.md §7: `sensor-current`), so `current` is 0 and
    // the environmental fields stay off the wire — never an invented value.
    #[cfg(not(feature = "sim"))]
    let mut rng = Xorshift64::new(seed_from(device_id.as_str(), seed));
    #[cfg(not(feature = "sim"))]
    let mut seq: u32 = 0;
    #[cfg(not(feature = "sim"))]
    println!("Telemetry: measured fields only (no simulator; current reports 0 until a sensor driver lands)");

    let mut backoff = BACKOFF_INITIAL;
    // OTA: the version the endpoint asked for on the last uplink, and when
    // a failed attempt allows the next one.
    let mut ota_target: Option<heapless::String<32>> = None;
    let mut ota_retry_after: Option<Instant> = None;

    // Outer loop: (re)establish TCP + TLS session.
    // Inner loop: reuse the session across uplinks — one handshake, many POSTs.
    loop {
        let address = match stack
            .dns_query(HOST, embassy_net::dns::DnsQueryType::A)
            .await
        {
            Ok(addresses) => {
                if let Some(first_addr) = addresses.first() {
                    *first_addr
                } else {
                    println!("No addresses returned from DNS query for host: {}", HOST);
                    backoff_delay(&mut backoff).await;
                    continue;
                }
            }
            Err(e) => {
                println!("DNS resolution failed for host {}: {:?}", HOST, e);
                backoff_delay(&mut backoff).await;
                continue;
            }
        };

        let remote_endpoint = (address, AWS_IOT_PORT);

        let client_conf = load_certificates();

        let mut socket = TcpSocket::new(stack, &mut rx_buffer[..], &mut tx_buffer[..]);
        socket.set_timeout(Some(SOCKET_TIMEOUT));

        if let Err(e) = socket.connect(remote_endpoint).await {
            println!("   TCP connect error: {:?}", e);
            backoff_delay(&mut backoff).await;
            continue;
        }

        let mut session = match Session::new(
            tls.reference(),
            socket,
            &SessionConfig::Client(client_conf.clone()),
        ) {
            Ok(s) => s,
            Err(e) => {
                println!("   Failed to create TLS session: {:?}", e);
                backoff_delay(&mut backoff).await;
                continue;
            }
        };

        match embassy_time::with_timeout(TLS_HANDSHAKE_TIMEOUT, session.connect()).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                println!("   TLS connect error: {:?}", e);
                backoff_delay(&mut backoff).await;
                continue;
            }
            Err(_) => {
                println!("   TLS connect timed out after 15 seconds");
                backoff_delay(&mut backoff).await;
                continue;
            }
        };

        backoff = BACKOFF_INITIAL;

        loop {
            let uptime_s = embassy_time::Instant::now()
                .as_secs()
                .min(u64::from(u32::MAX)) as u32;
            let heap_free = esp_alloc::HEAP.free().min(u32::MAX as usize) as u32;
            #[cfg(feature = "sim")]
            #[allow(unused_mut)]
            let mut uplink = sim.uplink(
                device_id.clone(),
                uptime_s,
                UPLINK_INTERVAL_S,
                UPLINK_BURST,
                Hardware {
                    heap_free: Some(heap_free),
                    rssi_dbm: None,
                },
            );
            #[cfg(not(feature = "sim"))]
            #[allow(unused_mut)]
            let mut uplink = {
                seq = seq.wrapping_add(1);
                let mut msg = UplinkMessage::new(device_id.clone(), 0);
                msg.seq = Some(seq);
                msg.uptime_s = Some(uptime_s);
                msg.interval_s = Some(UPLINK_INTERVAL_S);
                msg.heap_free = Some(heap_free);
                msg
            };
            // RSSI is a WiFi notion; wired boards leave it off the wire —
            // the scalar and the burst's register alike.
            #[cfg(not(feature = "wifi"))]
            {
                use supervictor_common::{fields, levels};
                uplink.rssi_dbm = None;
                if let Some(r) = levels::REGISTERS
                    .iter()
                    .position(|r| r.key == fields::UPLINK_RSSI_DBM)
                {
                    for sample in uplink.levels.chunks_mut(levels::COUNT) {
                        sample[r] = levels::ABSENT;
                    }
                }
            }

            let request = match post_request(HOST, &uplink, Some(API_PATH)) {
                Ok(r) => r,
                Err(e) => {
                    println!("   Failed to build HTTP request: {}", e);
                    break;
                }
            };

            if let Err(e) = session.write(request.as_bytes()).await {
                println!("   Write failed: {:?}", e);
                break;
            }

            match read_ack(&mut session).await {
                Ack::Framed { status, body } => {
                    // A completed round-trip is the liveness proof the
                    // watchdog contract wants — feed nowhere else.
                    rwdt.feed();
                    #[cfg(debug_assertions)]
                    match core::str::from_utf8(&body) {
                        Ok(s) => println!("Received {} response:\n---\n{}\n---", status, s),
                        Err(_) => println!("   Response {} not UTF-8", status),
                    }
                    #[cfg(not(debug_assertions))]
                    println!("   Response {} ({} body bytes)", status, body.len());
                    if (200..300).contains(&status) {
                        // This firmware phoned home: if it was an OTA
                        // candidate, it is now the firmware.
                        if let Some(o) = ota.as_mut() {
                            o.confirm();
                        }
                        let ack = core::str::from_utf8(&body)
                            .map(UplinkAck::parse)
                            .unwrap_or_default();
                        if let Some(desired) = ack.desired_fw {
                            let due = ota_retry_after.is_none_or(|t| Instant::now() >= t);
                            if ota.is_some() && Ota::wants(desired.as_str()) && due {
                                ota_target = Some(desired);
                                break;
                            }
                        }
                    }
                }
                Ack::Closed => {
                    // Peer closed the connection (keepalive expired).
                    println!("   Peer closed connection — reconnecting");
                    break;
                }
                Ack::Broken => break,
            }

            // ±20 % jitter so boards flashed together don't phase-lock.
            #[cfg(feature = "sim")]
            let delay_ms = sim.next_delay_ms(MAIN_LOOP_DELAY.as_millis() as u32);
            #[cfg(not(feature = "sim"))]
            let delay_ms = jitter_ms(&mut rng, MAIN_LOOP_DELAY.as_millis() as u32);
            Timer::after(Duration::from_millis(u64::from(delay_ms))).await;
        }

        let _ = session.close().await;
        drop(session);

        // The uplink session is down and its buffers are free: run the
        // update on them. On success this reboots; on failure the uplink
        // loop resumes and leaves the version alone for a while.
        if let (Some(target), Some(o)) = (ota_target.take(), ota.as_mut()) {
            match o
                .update(
                    stack,
                    tls.reference(),
                    &mut rx_buffer[..],
                    &mut tx_buffer[..],
                    target.as_str(),
                )
                .await
            {
                Ok(never) => match never {},
                Err(e) => {
                    println!(
                        "OTA: update to {} failed: {:?} — next try in {}s",
                        target,
                        e,
                        OTA_RETRY_DELAY.as_secs()
                    );
                    ota_retry_after = Some(Instant::now() + OTA_RETRY_DELAY);
                }
            }
        }
    }
}
