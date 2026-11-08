//! Provisioning portal: when a WiFi board has no network to join, or
//! could not join the one it has, it becomes an access point and serves
//! one page where SSID, password, endpoint host and device name are
//! entered. The board joins the network and completes a TLS handshake
//! with the host *before* writing anything; then it saves the record and
//! reboots as a station. Decided 2026-11-02 (interview with Nick).
//!
//! - Access point `supervictor-<device name>`, WPA2, passphrase = last
//!   eight hex digits of the device certificate's SHA-256 fingerprint
//!   ([`psk`]); `flash.sh` prints it, so does the console at boot.
//! - 192.168.4.1/24, a DHCP server ([`dhcp`]) and a DNS server that
//!   answers everything with the board's address (`network::dns`), so a
//!   phone's captive-portal detector opens the form by itself.
//! - Opens on a blank record, or when the previous boot never associated
//!   within the watchdog window ([`boot`]). Closes by rebooting.
//!
//! [`page`], [`dhcp`], [`psk`] and [`boot`] are pure and host-tested; the
//! tasks here need the radio and build only with `wifi`.

/// Whether this boot runs the portal (RTC record policy).
pub mod boot;
/// DISCOVER/REQUEST → OFFER/ACK for the portal's clients.
pub mod dhcp;
/// The form, its decoding, the responses.
pub mod page;
/// WPA2 passphrase from the device certificate.
pub mod psk;

#[cfg(test)]
mod boot_tests;
#[cfg(test)]
mod dhcp_tests;
#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod psk_tests;

#[cfg(feature = "wifi")]
pub use embedded::*;

#[cfg(feature = "wifi")]
mod embedded {
    use embassy_executor::Spawner;
    use embassy_net::tcp::TcpSocket;
    use embassy_net::udp::{PacketMetadata, UdpSocket};
    use embassy_net::{
        Config, Ipv4Address, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4,
    };
    use embassy_time::{with_timeout, Duration, Instant, Timer};
    use esp_hal::rng::Trng;
    use esp_hal::rtc_cntl::Rwdt;
    use esp_println::println;
    use esp_radio::wifi::{
        self, AccessPointConfig, AuthMethod, ClientConfig, ModeConfig, WifiController, WifiDevice,
    };
    use mbedtls_rs::{Session, SessionConfig, Tls};
    use static_cell::StaticCell;

    use super::boot::{self, Mark, Reason};
    use super::dhcp::{self, Leases};
    use super::page::{self, Outcome, Route, PAGE_MAX, REQUEST_MAX};
    use super::psk;
    use crate::config::{AWS_IOT_PORT, SOCKET_TIMEOUT, TLS_HANDSHAKE_TIMEOUT};
    use crate::models::uplink::FIRMWARE_VERSION;
    use crate::network::server::{extract_body, parse_request_line};
    use crate::network::tls::{load_certificates, CLIENT_CERT_PEM};
    use crate::ota::device::Ota;
    use crate::settings::device::Store;
    use crate::settings::Settings;

    /// Clients the access point admits at once.
    const AP_MAX_CLIENTS: u16 = 4;
    /// How long a candidate network gets to associate.
    const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
    /// How long a lease gets to arrive once associated.
    const DHCP_TIMEOUT: Duration = Duration::from_secs(15);
    /// Longest the portal waits for a client before feeding the watchdog
    /// and listening again. Well inside `WATCHDOG_TIMEOUT_S`.
    const ACCEPT_SLICE: Duration = Duration::from_secs(60);
    /// Longest one request may take to arrive in full.
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
    /// With a network configured but unreachable, give it another try
    /// after this long with nobody on the portal.
    const IDLE_RETRY: Duration = Duration::from_secs(10 * 60);
    /// How long the success page stays up after a save before the board
    /// reboots as a station: long enough to rejoin the access point after
    /// the probe's channel switch dropped the client.
    const SAVED_GRACE: Duration = Duration::from_secs(60);

    /// The record that survives a watchdog reset (see `boot`).
    #[esp_hal::ram(unstable(rtc_fast, persistent))]
    static mut PORTAL_RTC: [u32; boot::WORDS] = [0; boot::WORDS];

    fn rtc_load() -> Option<Mark> {
        // SAFETY: single-core firmware; the only accesses are these two
        // helpers, each short-lived and never overlapping.
        let words = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PORTAL_RTC)) };
        boot::load(&words)
    }

    fn rtc_store(mark: Mark) {
        let mut words = [0u32; boot::WORDS];
        boot::store(&mut words, mark);
        // SAFETY: see `rtc_load`.
        unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PORTAL_RTC), words) };
    }

    /// Station boot: record that it started. Call first thing on the WiFi
    /// path; `joined` upgrades it on association.
    pub fn mark_booted() {
        rtc_store(Mark::Booted);
    }

    /// The station associated: this boot is not a failed join.
    pub fn mark_joined() {
        rtc_store(Mark::Joined);
    }

    /// Open the portal this boot? Reads what the previous boot left.
    pub fn should_open(settings: &Settings) -> Option<Reason> {
        boot::decide(rtc_load(), settings.has_wifi())
    }

    macro_rules! make_static {
        ($t:ty, $val:expr) => {{
            static CELL: StaticCell<$t> = StaticCell::new();
            CELL.uninit().write($val)
        }};
    }

    #[embassy_executor::task]
    async fn ap_net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
        runner.run().await
    }

    #[embassy_executor::task]
    async fn sta_net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
        runner.run().await
    }

    /// DHCP for the portal's clients: one address each from a short table.
    #[embassy_executor::task]
    async fn dhcp_task(stack: Stack<'static>) {
        let rx_meta = make_static!([PacketMetadata; 4], [PacketMetadata::EMPTY; 4]);
        let tx_meta = make_static!([PacketMetadata; 4], [PacketMetadata::EMPTY; 4]);
        let rx_buf = make_static!([u8; 1024], [0; 1024]);
        let tx_buf = make_static!([u8; 1024], [0; 1024]);
        let mut socket = UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf);
        if socket.bind(67).is_err() {
            println!("portal: DHCP bind failed — clients must use a static address");
            return;
        }
        let mut leases = Leases::new();
        let mut buf = [0u8; 600];
        loop {
            let Ok((n, _from)) = socket.recv_from(&mut buf).await else {
                continue;
            };
            if let Some(reply) = dhcp::handle(&buf[..n], &mut leases) {
                let kind = dhcp::reply_type(&reply).unwrap_or(0);
                let _ = socket.send_to(&reply, (Ipv4Address::BROADCAST, 68)).await;
                println!(
                    "portal: DHCP {} for {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} -> {}.{}.{}.{}",
                    match kind {
                        2 => "offer",
                        5 => "ack",
                        6 => "nak",
                        _ => "reply",
                    },
                    buf[28],
                    buf[29],
                    buf[30],
                    buf[31],
                    buf[32],
                    buf[33],
                    reply[16],
                    reply[17],
                    reply[18],
                    reply[19]
                );
            }
        }
    }

    /// DNS that answers every name with the portal, so the phone's
    /// connectivity probe lands on the form.
    #[embassy_executor::task]
    async fn dns_task(stack: Stack<'static>) {
        let rx_meta = make_static!([PacketMetadata; 4], [PacketMetadata::EMPTY; 4]);
        let tx_meta = make_static!([PacketMetadata; 4], [PacketMetadata::EMPTY; 4]);
        let rx_buf = make_static!([u8; 1024], [0; 1024]);
        let tx_buf = make_static!([u8; 1024], [0; 1024]);
        let mut socket = UdpSocket::new(stack, rx_meta, rx_buf, tx_meta, tx_buf);
        if socket.bind(53).is_err() {
            println!("portal: DNS bind failed — open http://192.168.4.1/ by address");
            return;
        }
        let mut buf = [0u8; 512];
        loop {
            let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                continue;
            };
            if let Some(reply) = crate::network::dns::build_dns_response(&buf[..n]) {
                let _ = socket.send_to(&reply, from).await;
            }
        }
    }

    /// Why the candidate was not saved, mapped onto the page.
    #[derive(Debug, Clone, Copy)]
    enum ProbeError {
        Join,
        Dhcp,
        Dns,
        Tcp,
        Tls,
    }

    impl From<ProbeError> for Outcome {
        fn from(e: ProbeError) -> Outcome {
            match e {
                ProbeError::Join => Outcome::Join,
                ProbeError::Dhcp => Outcome::Dhcp,
                ProbeError::Dns => Outcome::Dns,
                ProbeError::Tcp => Outcome::Tcp,
                ProbeError::Tls => Outcome::Tls,
            }
        }
    }

    /// Join the candidate network and complete one mTLS handshake with
    /// its host, leaving the access point up throughout. Disconnects the
    /// station again either way.
    async fn probe(
        controller: &mut WifiController<'static>,
        sta: Stack<'static>,
        tls: &mut Tls<'static>,
        ap: &AccessPointConfig,
        candidate: &Settings,
        rx: &mut [u8],
        tx: &mut [u8],
    ) -> Result<(), ProbeError> {
        let client = ClientConfig::default()
            .with_ssid(candidate.ssid.as_str().into())
            .with_password(candidate.password.as_str().into());
        controller
            .set_config(&ModeConfig::ApSta(client, ap.clone()))
            .map_err(|_| ProbeError::Join)?;
        let result = probe_inner(controller, sta, tls, candidate, rx, tx).await;
        let _ = controller.disconnect_async().await;
        result
    }

    async fn probe_inner(
        controller: &mut WifiController<'static>,
        sta: Stack<'static>,
        tls: &mut Tls<'static>,
        candidate: &Settings,
        rx: &mut [u8],
        tx: &mut [u8],
    ) -> Result<(), ProbeError> {
        println!("portal: joining {} …", candidate.ssid.as_str());
        match with_timeout(JOIN_TIMEOUT, controller.connect_async()).await {
            Ok(Ok(())) => {}
            _ => return Err(ProbeError::Join),
        }
        println!("portal: associated; waiting for a lease …");
        if with_timeout(DHCP_TIMEOUT, sta.wait_config_up())
            .await
            .is_err()
        {
            return Err(ProbeError::Dhcp);
        }
        let host = candidate.host.as_str();
        println!("portal: resolving {} …", host);
        let address = sta
            .dns_query(host, embassy_net::dns::DnsQueryType::A)
            .await
            .ok()
            .and_then(|a| a.first().copied())
            .ok_or(ProbeError::Dns)?;
        let mut socket = TcpSocket::new(sta, rx, tx);
        socket.set_timeout(Some(SOCKET_TIMEOUT));
        socket
            .connect((address, AWS_IOT_PORT))
            .await
            .map_err(|_| ProbeError::Tcp)?;
        println!("portal: TCP up; TLS handshake with {} …", host);
        let config = load_certificates(candidate.host.as_cstr());
        let mut session = Session::new(tls.reference(), socket, &SessionConfig::Client(config))
            .map_err(|_| ProbeError::Tls)?;
        match with_timeout(TLS_HANDSHAKE_TIMEOUT, session.connect()).await {
            Ok(Ok(())) => {}
            _ => return Err(ProbeError::Tls),
        }
        let _ = session.close().await;
        println!("portal: handshake complete — {} trusts this board", host);
        Ok(())
    }

    /// Read one HTTP request: the head, then `Content-Length` bytes of
    /// body. `None` on timeout, overflow or a closed connection.
    async fn read_request(
        socket: &mut TcpSocket<'_>,
        buf: &mut [u8; REQUEST_MAX],
    ) -> Option<usize> {
        let mut filled = 0usize;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut body_wanted: Option<usize> = None;
        loop {
            if let Some(head_end) = find_head_end(&buf[..filled]) {
                let wanted = *body_wanted.get_or_insert_with(|| {
                    core::str::from_utf8(&buf[..head_end])
                        .map(page::content_length)
                        .unwrap_or(0)
                });
                if filled >= head_end + 4 + wanted {
                    return Some(head_end + 4 + wanted);
                }
            }
            if filled == buf.len() {
                return None;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match with_timeout(left, socket.read(&mut buf[filled..])).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return None,
                Ok(Ok(n)) => filled += n,
            }
        }
    }

    fn find_head_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }

    async fn send_all(socket: &mut TcpSocket<'_>, mut data: &[u8]) {
        while !data.is_empty() {
            match socket.write(data).await {
                Ok(0) | Err(_) => return,
                Ok(n) => data = &data[n..],
            }
        }
        let _ = socket.flush().await;
    }

    /// Run the portal. Never returns: the board reboots when settings are
    /// saved (or when a configured network gets another try).
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        spawner: Spawner,
        radio: &'static esp_radio::Controller<'static>,
        wifi_periph: esp_hal::peripherals::WIFI<'static>,
        settings: &'static Settings,
        mut ota: Option<&'static mut Ota>,
        mut rwdt: Rwdt,
        seed: u64,
        reason: Reason,
    ) -> ! {
        rtc_store(Mark::Portal);
        rwdt.feed();

        let ap_ssid = page::ap_ssid(settings.device_name.as_str());
        let passphrase = psk::psk_from_pem(CLIENT_CERT_PEM);
        let mut ap = AccessPointConfig::default()
            .with_ssid(ap_ssid.as_str().into())
            .with_max_connections(AP_MAX_CLIENTS);
        ap = match passphrase.as_ref() {
            Some(p) => ap
                .with_auth_method(AuthMethod::Wpa2Personal)
                .with_password(p.as_str().into()),
            None => {
                println!("portal: could not derive a passphrase from the device certificate — OPEN access point");
                ap.with_auth_method(AuthMethod::None)
            }
        };
        println!(
            "portal: {:?} — access point {} ({}), then http://192.168.4.1/",
            reason,
            ap_ssid.as_str(),
            match passphrase.as_ref() {
                Some(p) => p.as_str(),
                None => "open",
            }
        );

        let (mut controller, interfaces) =
            match wifi::new(radio, wifi_periph, wifi::Config::default()) {
                Ok(r) => r,
                Err(e) => {
                    println!("portal: FATAL WiFi init failed: {:?}", e);
                    halt().await
                }
            };
        if let Err(e) =
            controller.set_config(&ModeConfig::ApSta(ClientConfig::default(), ap.clone()))
        {
            println!("portal: FATAL set_config failed: {:?}", e);
            halt().await
        }
        if let Err(e) = controller.start_async().await {
            println!("portal: FATAL WiFi start failed: {:?}", e);
            halt().await
        }

        let ap_ip = Ipv4Address::new(192, 168, 4, 1);
        let mut dns_servers = heapless::Vec::new();
        let _ = dns_servers.push(ap_ip);
        let (ap_stack, ap_runner) = embassy_net::new(
            interfaces.ap,
            Config::ipv4_static(StaticConfigV4 {
                address: Ipv4Cidr::new(ap_ip, 24),
                gateway: Some(ap_ip),
                dns_servers,
            }),
            make_static!(StackResources<4>, StackResources::<4>::new()),
            seed,
        );
        let (sta_stack, sta_runner) = embassy_net::new(
            interfaces.sta,
            Config::dhcpv4(Default::default()),
            make_static!(StackResources<3>, StackResources::<3>::new()),
            seed ^ 0x5bd1_e995,
        );
        spawner.spawn(ap_net_task(ap_runner).unwrap());
        spawner.spawn(sta_net_task(sta_runner).unwrap());
        spawner.spawn(dhcp_task(ap_stack).unwrap());
        spawner.spawn(dns_task(ap_stack).unwrap());

        let trng = match Trng::try_new() {
            Ok(t) => t,
            Err(e) => {
                println!("portal: FATAL TRNG: {:?}", e);
                halt().await
            }
        };
        let mut tls = match Tls::new(make_static!(Trng, trng)) {
            Ok(t) => t,
            Err(e) => {
                println!("portal: FATAL TLS context: {:?}", e);
                halt().await
            }
        };
        tls.set_debug(0);

        let http_rx = make_static!([u8; 2048], [0; 2048]);
        let http_tx = make_static!([u8; PAGE_MAX + 128], [0; PAGE_MAX + 128]);
        let probe_rx = make_static!([u8; 4096], [0; 4096]);
        let probe_tx = make_static!([u8; 4096], [0; 4096]);
        let request = make_static!([u8; REQUEST_MAX], [0; REQUEST_MAX]);

        let mut current = settings.clone();
        let mut outcome = Outcome::Fresh;
        let mut last_request = Instant::now();
        // Set once a probe passed and the record is written: the success
        // page stays up for SAVED_GRACE, then the board reboots.
        let mut saved: Option<(Settings, Instant)> = None;
        loop {
            rwdt.feed();
            if let Some((_, at)) = saved.as_ref() {
                if at.elapsed() >= SAVED_GRACE {
                    println!("portal: rebooting as a station");
                    rtc_store(Mark::Portal);
                    esp_hal::system::software_reset();
                }
            } else if reason == Reason::JoinFailed && last_request.elapsed() > IDLE_RETRY {
                println!(
                    "portal: nobody came in {} s — trying the configured network again",
                    IDLE_RETRY.as_secs()
                );
                esp_hal::system::software_reset();
            }
            let slice = match saved.as_ref() {
                Some((_, at)) => SAVED_GRACE
                    .checked_sub(at.elapsed())
                    .unwrap_or(Duration::from_millis(1))
                    .max(Duration::from_millis(1)),
                None => ACCEPT_SLICE,
            };
            let mut socket = TcpSocket::new(ap_stack, &mut http_rx[..], &mut http_tx[..]);
            socket.set_timeout(Some(REQUEST_TIMEOUT));
            if with_timeout(slice, socket.accept(80)).await.is_err() {
                continue;
            }
            last_request = Instant::now();
            let Some(len) = read_request(&mut socket, request).await else {
                socket.abort();
                let _ = socket.flush().await;
                continue;
            };
            let text = core::str::from_utf8(&request[..len]).unwrap_or("");
            let (method, path) = parse_request_line(text);
            let route = page::route(method, path);
            if let Some((done, _)) = saved.as_ref() {
                // Settled: every page is the confirmation until the reboot.
                let body = match route {
                    Route::Redirect => None,
                    _ => Some(page::saved_page(settings.device_name.as_str(), done)),
                };
                match body {
                    Some(b) => {
                        send_all(&mut socket, page::html_response(b.as_str()).as_bytes()).await
                    }
                    None => send_all(&mut socket, page::redirect_response().as_bytes()).await,
                }
            } else {
                match route {
                    Route::Form => {
                        let body = page::form_page(
                            settings.device_name.as_str(),
                            FIRMWARE_VERSION,
                            &current,
                            outcome,
                        );
                        send_all(&mut socket, page::html_response(body.as_str()).as_bytes()).await;
                    }
                    Route::Redirect => {
                        send_all(&mut socket, page::redirect_response().as_bytes()).await;
                    }
                    Route::Configure => match page::parse_form(extract_body(text)) {
                        Err(page::FormError::Field(f)) => {
                            outcome = Outcome::BadForm(f);
                            let body = page::form_page(
                                settings.device_name.as_str(),
                                FIRMWARE_VERSION,
                                &current,
                                outcome,
                            );
                            send_all(&mut socket, page::html_response(body.as_str()).as_bytes())
                                .await;
                        }
                        Ok(candidate) => {
                            // Answer before trying the network: the probe moves
                            // the radio to its channel and drops this client.
                            let body =
                                page::checking_page(settings.device_name.as_str(), &candidate);
                            send_all(&mut socket, page::html_response(body.as_str()).as_bytes())
                                .await;
                            socket.close();
                            let _ = socket.flush().await;
                            Timer::after(Duration::from_millis(300)).await;
                            socket.abort();
                            let _ = socket.flush().await;
                            drop(socket);
                            current = Settings {
                                password: heapless::String::new(),
                                ..candidate.clone()
                            };
                            rwdt.feed();
                            let result = probe(
                                &mut controller,
                                sta_stack,
                                &mut tls,
                                &ap,
                                &candidate,
                                &mut probe_rx[..],
                                &mut probe_tx[..],
                            )
                            .await;
                            rwdt.feed();
                            match result {
                                Err(e) => {
                                    outcome = e.into();
                                    println!(
                                        "portal: {} not saved: {:?} — shown on the next page load",
                                        candidate.ssid.as_str(),
                                        outcome
                                    );
                                }
                                Ok(()) => {
                                    let written = match ota.as_mut() {
                                        Some(o) => o.with_flash(|flash, pt| {
                                            Store::open(flash, pt)
                                                .and_then(|mut s| s.write(flash, pt, &candidate))
                                        }),
                                        None => {
                                            Err(crate::settings::device::StoreError::NoPartition)
                                        }
                                    };
                                    match written {
                                        Ok(()) => {
                                            println!(
                                                "portal: saved (ssid {}, host {}, device {}); confirmation up for {} s, then a station",
                                                candidate.ssid.as_str(),
                                                candidate.host.as_str(),
                                                candidate.device_name.as_str(),
                                                SAVED_GRACE.as_secs()
                                            );
                                            saved = Some((candidate, Instant::now()));
                                        }
                                        Err(e) => {
                                            println!("portal: flash write failed: {:?}", e);
                                            outcome = Outcome::Flash;
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                    },
                }
            }
            socket.close();
            let _ = socket.flush().await;
            Timer::after(Duration::from_millis(50)).await;
            socket.abort();
            let _ = socket.flush().await;
        }
    }

    async fn halt() -> ! {
        loop {
            Timer::after(Duration::from_secs(60)).await;
        }
    }
}
