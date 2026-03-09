use crate::config::*;
use crate::models::uplink::UplinkMessage;
use crate::network::http::post_request;
use crate::network::tls::load_certificates;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, Stack};
use embassy_time::{Duration, Timer};
use esp_println::println;
use esp_radio::wifi::{WifiController, WifiDevice, WifiEvent, WifiStaState};
use mbedtls_rs::{Session, SessionConfig, Tls};

#[cfg(feature = "embedded")]
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

#[cfg(feature = "embedded")]
#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

#[cfg(feature = "embedded")]
#[embassy_executor::task]
pub async fn app(stack: Stack<'static>, tls: Tls<'static>) {
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

    let uplink = UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    };

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
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            }
            Err(e) => {
                println!("DNS resolution failed for host {}: {:?}", HOST, e);
                Timer::after(MAIN_LOOP_DELAY).await;
                continue;
            }
        };

        let remote_endpoint = (address, AWS_IOT_PORT);

        let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
        let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

        let client_conf = load_certificates();

        loop {
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(SOCKET_TIMEOUT));

            if let Err(e) = socket.connect(remote_endpoint).await {
                println!("   TCP connect error: {:?}", e);
                Timer::after(MAIN_LOOP_DELAY).await;
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
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };

            match embassy_time::with_timeout(TLS_HANDSHAKE_TIMEOUT, session.connect()).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    println!("   TLS connect error: {:?}", e);
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
                Err(_) => {
                    println!("   TLS connect timed out after 15 seconds");
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };

            let request = match post_request(env!("HOST"), &uplink, None) {
                Ok(r) => r,
                Err(e) => {
                    println!("   Failed to build HTTP request: {}", e);
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };
            match session.write(request.as_bytes()).await {
                Ok(written) => {
                    if written != request.len() {
                        println!("   Only wrote {} of {} bytes", written, request.len());
                    }
                }
                Err(e) => println!("   Failed to send request: {:?}", e),
            };

            // Try to read response
            let mut buffer = [0u8; 1024];
            match embassy_time::with_timeout(HTTP_READ_TIMEOUT, session.read(&mut buffer)).await {
                Ok(Ok(n)) => {
                    if n > 0 {
                        #[cfg(debug_assertions)]
                        match core::str::from_utf8(&buffer[..n]) {
                            Ok(s) => println!("Received response:\n---\n{}\n---", s),
                            Err(_) => println!("   Response not UTF-8 (binary data)"),
                        }
                        #[cfg(not(debug_assertions))]
                        println!("   Response received ({} bytes)", n);
                    } else {
                        println!("   Empty response (0 bytes)");
                    }
                }
                Ok(Err(e)) => println!("   Read failed: {:?}", e),
                Err(_) => println!("   Read timed out"),
            };
            Timer::after(MAIN_LOOP_DELAY).await;
        }
    }
}
