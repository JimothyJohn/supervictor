use crate::config::*;
use crate::models::sim::{seed_from, Hardware, Simulator};
use crate::models::uplink::UplinkMessage;
use crate::network::http::post_request;
use crate::network::tls::load_certificates;
use embassy_net::tcp::TcpSocket;
#[cfg(feature = "wifi")]
use embassy_net::Runner;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
#[cfg(feature = "embedded")]
use esp_hal::rtc_cntl::Rwdt;
use esp_println::println;
#[cfg(feature = "wifi")]
use esp_radio::wifi::{WifiController, WifiDevice, WifiEvent, WifiStaState};
use mbedtls_rs::{Session, SessionConfig, Tls};

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

/// Main application loop: resolves DNS, establishes mTLS, and sends uplink telemetry.
#[cfg(feature = "embedded")]
#[embassy_executor::task]
pub async fn app(stack: Stack<'static>, tls: Tls<'static>, mut rwdt: Rwdt) {
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
    const _: () = assert!(env!("DEVICE_NAME").len() <= 64);
    let mut uplink = UplinkMessage::new(env!("DEVICE_NAME").try_into().unwrap(), 100);
    uplink.interval_s = Some(UPLINK_INTERVAL_S);

    // Simulated readings until real sensors land (docs/TELEMETRY.md): a
    // per-board personality from DEVICE_NAME, salted by the hardware RNG so
    // two boots don't replay the same trace. Heap and uptime stay real.
    let rng = esp_hal::rng::Rng::new();
    let salt = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    let mut sim = Simulator::new(seed_from(env!("DEVICE_NAME"), salt));

    // Uplink path from API_PATH at compile time: "/" against a custom domain
    // (base-path mapping supplies the stage), "/<stage>" against a raw
    // execute-api URL. Must start with '/'.
    const API_PATH: &str = env!("API_PATH");
    const _: () = assert!(!API_PATH.is_empty() && API_PATH.as_bytes()[0] == b'/');

    let mut backoff = BACKOFF_INITIAL;

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

        let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
        let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];
        let client_conf = load_certificates();

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
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
            // Refresh health metadata for this ping; sensor fields come from
            // the simulator (real heap wins over its simulated leak).
            let heap_free = esp_alloc::HEAP.free() as u32;
            let sample = sim.sample(Hardware {
                heap_free: Some(heap_free),
                rssi_dbm: None,
            });
            uplink.seq = Some(uplink.seq.map_or(1, |n| n.wrapping_add(1)));
            uplink.uptime_s = Some(embassy_time::Instant::now().as_secs() as u32);
            uplink.heap_free = Some(heap_free);
            uplink.current = sample.current;
            uplink.temp_c = Some(sample.temp_c);
            uplink.humidity_pct = Some(sample.humidity_pct);
            // RSSI is a WiFi notion; wired boards leave it off the wire.
            #[cfg(feature = "wifi")]
            {
                uplink.rssi_dbm = Some(sample.rssi_dbm);
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

            let mut buffer = [0u8; 1024];
            match embassy_time::with_timeout(HTTP_READ_TIMEOUT, session.read(&mut buffer)).await {
                Ok(Ok(n)) if n > 0 => {
                    // A completed round-trip is the liveness proof the
                    // watchdog contract wants — feed nowhere else.
                    rwdt.feed();
                    #[cfg(debug_assertions)]
                    match core::str::from_utf8(&buffer[..n]) {
                        Ok(s) => println!("Received response:\n---\n{}\n---", s),
                        Err(_) => println!("   Response not UTF-8 (binary data)"),
                    }
                    #[cfg(not(debug_assertions))]
                    println!("   Response received ({} bytes)", n);
                }
                Ok(Ok(_)) => {
                    // 0-byte read = peer closed the connection (keepalive expired).
                    println!("   Peer closed connection — reconnecting");
                    break;
                }
                Ok(Err(e)) => {
                    println!("   Read failed: {:?}", e);
                    break;
                }
                Err(_) => {
                    println!("   Read timed out");
                    break;
                }
            }

            // ±20 % jitter so boards flashed together don't phase-lock.
            let delay_ms = sim.next_delay_ms(MAIN_LOOP_DELAY.as_millis() as u32);
            Timer::after(Duration::from_millis(u64::from(delay_ms))).await;
        }

        let _ = session.close().await;
    }
}
