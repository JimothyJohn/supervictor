use crate::config::*;
use crate::models::uplink::UplinkMessage;
use crate::network::http::post_request;
use crate::network::tls::load_certificates;
use core::ffi::CStr;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, Stack};
use embassy_time::{Duration, Timer};
use esp_mbedtls::{asynch::Session, Mode, Tls, TlsVersion};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};

#[cfg(feature = "embedded")]
#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    loop {
        if esp_wifi::wifi::wifi_state() == WifiState::StaConnected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let Ok(ssid_hs) = ssid.try_into() else {
                println!("SSID too long for heapless buffer, retrying...");
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            };
            let Ok(password_hs) = password.try_into() else {
                println!("Password too long for heapless buffer, retrying...");
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            };
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid_hs,
                password: password_hs,
                ..Default::default()
            });
            if let Err(e) = controller.set_configuration(&client_config) {
                println!("WiFi set_configuration failed: {e:?}");
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
            if let Err(e) = controller.start_async().await {
                println!("WiFi start_async failed: {e:?}");
                Timer::after(Duration::from_millis(5000)).await;
                continue;
            }
        }

        match controller.connect_async().await {
            Ok(_) => println!("Connected to Wifi!"),
            Err(e) => {
                println!("Failed to connect to Wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
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

        // These need to be mutable and their lifetime is tied to the socket.
        let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
        let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

        // However, keeping it here simplifies state management per connection attempt.
        let certs = load_certificates();

        // This ensures the CStr reference lives long enough for the Session::new call.
        let host_bytes_with_null = concat!(env!("HOST"), "\0").as_bytes();
        let host_cstr = match CStr::from_bytes_with_nul(&host_bytes_with_null) {
            Ok(cstr) => cstr,
            Err(e) => {
                println!("   ❌ FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                        env!("HOST"), e);
                panic!("FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                        env!("HOST"), e);
            }
        };

        loop {
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(SOCKET_TIMEOUT));

            if let Err(e) = socket.connect(remote_endpoint).await {
                println!("   ❌ TCP connect error: {:?}", e);
                // socket.close();
                Timer::after(MAIN_LOOP_DELAY).await;
                continue;
            }

            // The 'socket' is moved into the Session here.
            let mut session = match Session::new(
                &mut socket,
                Mode::Client {
                    servername: host_cstr,
                },
                match option_env!("TLS_VERSION") {
                    Some("1.3") => TlsVersion::Tls1_3,
                    _ => TlsVersion::Tls1_2,
                },
                certs,
                tls.reference(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    println!("   ❌ Failed to create TLS session: {:?}", e);
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };

            match embassy_time::with_timeout(
                TLS_HANDSHAKE_TIMEOUT, // 15 second timeout
                session.connect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    println!("   ❌ TLS connect error: {:?}", e);
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
                Err(_) => {
                    println!("   ❌ TLS connect timed out after 15 seconds");
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };

            let request = match post_request(env!("HOST"), &uplink, None) {
                Ok(r) => r,
                Err(e) => {
                    println!("   ❌ Failed to build HTTP request: {}", e);
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue;
                }
            };
            match session.write(request.as_bytes()).await {
                Ok(written) => {
                    if written != request.len() {
                        println!("   ⚠️ Only wrote {} of {} bytes", written, request.len());
                    }
                }
                Err(e) => println!("   ❌ Failed to send request: {:?}", e),
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
                Ok(Err(e)) => println!("   ❌ Read failed: {:?}", e),
                Err(_) => println!("   ❌ Read timed out"),
            };
            Timer::after(MAIN_LOOP_DELAY).await;
        }
    }
}
