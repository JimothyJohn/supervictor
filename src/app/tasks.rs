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
use heapless::String as HString;

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
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            controller.start_async().await.unwrap();
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

    // AI-Generated comment: Call the function to create and serialize the data map.
    let json_body: HString<128> = match serde_json_core::to_string(&UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    }) {
        Ok(body) => body,
        Err(e) => {
            println!("Error serializing JSON: {:?}", e);
            let json_body: HString<128> = "{}".try_into().unwrap();
            json_body
        }
    };

    // AI-Generated comment: The main application loop. Each iteration will attempt to make a new connection and send a request.
    loop {
        // AI-Generated comment: DNS resolution is performed in each loop iteration in case the IP changes.
        let address = match stack
            .dns_query(HOST, embassy_net::dns::DnsQueryType::A)
            .await
        {
            Ok(addresses) => {
                if let Some(first_addr) = addresses.first() {
                    *first_addr
                } else {
                    println!("No addresses returned from DNS query for host: {}", HOST);
                    // AI-Generated comment: Delay before retrying DNS to avoid spamming queries on persistent failure.
                    Timer::after(MAIN_LOOP_DELAY).await;
                    continue; // AI-Generated comment: Skip to the next iteration of the loop.
                }
            }
            Err(e) => {
                println!("DNS resolution failed for host {}: {:?}", HOST, e);
                // AI-Generated comment: Delay before retrying DNS.
                Timer::after(MAIN_LOOP_DELAY).await;
                continue; // AI-Generated comment: Skip to the next iteration of the loop.
            }
        };

        let remote_endpoint = (address, AWS_IOT_PORT);

        // AI-Generated comment: Buffers for the TCP socket are created in each iteration.
        // These need to be mutable and their lifetime is tied to the socket.
        let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
        let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

        // AI-Generated comment: Load certificates. This could potentially be done outside the loop if certs don't change.
        // However, keeping it here simplifies state management per connection attempt.
        let certs = load_certificates();

        // AI-Generated comment: Create the CStr for the servername *before* Session::new.
        // This ensures the CStr reference lives long enough for the Session::new call.
        let host_bytes_with_null = concat!(env!("HOST"), "\0").as_bytes();
        let host_cstr = match CStr::from_bytes_with_nul(&host_bytes_with_null) {
            Ok(cstr) => cstr,
            Err(e) => {
                // AI-Generated comment: Log and panic if HOST env var is invalid (contains null bytes). This is a fatal configuration error.
                println!("   ❌ FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                        env!("HOST"), e);
                panic!("FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                        env!("HOST"), e);
            }
        };

        loop {
            // AI-Generated comment: A new TCP socket is created for each connection attempt.
            let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(SOCKET_TIMEOUT));

            // AI-Generated comment: Attempt to connect the TCP socket.
            if let Err(e) = socket.connect(remote_endpoint).await {
                println!("   ❌ TCP connect error: {:?}", e);
                // AI-Generated comment: Close the socket explicitly on error, though it might be implicitly closed on drop.
                // socket.close(); // AI-Generated comment: esp-hal's TcpSocket doesn't have an explicit close, relies on drop.
                Timer::after(MAIN_LOOP_DELAY).await; // AI-Generated comment: Wait before retrying.
                continue; // AI-Generated comment: Skip to the next iteration of the loop.
            }

            // AI-Generated comment: Initialize the TLS session for each new connection.
            // The 'socket' is moved into the Session here.
            let mut session = match Session::new(
                &mut socket,
                Mode::Client {
                    servername: host_cstr, // AI-Generated comment: Pass the host_cstr variable here.
                },
                TlsVersion::Tls1_3, // Using TLS 1.3 as per the code
                certs,              // AI-Generated comment: Pass the loaded certificates.
                tls.reference(),    // AI-Generated comment: Pass a reference to the Tls context.
            ) {
                Ok(s) => s,
                Err(e) => {
                    println!("   ❌ Failed to create TLS session: {:?}", e);
                    panic!("Failed to create TLS session: {:?}", e);
                }
            };

            // AI-Generated comment: Connect with timeout handling.
            match embassy_time::with_timeout(
                TLS_HANDSHAKE_TIMEOUT, // 15 second timeout
                session.connect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    println!("TLS connect error: {:?}", e);
                    panic!("TLS connect error: {:?}", e);
                }
                Err(_) => {
                    println!("TLS connect timed out after 15 seconds");
                    panic!("TLS connect timed out after 15 seconds");
                }
            };

            // Try sending a simple HTTP request to verify the connection
            let request = post_request(env!("HOST"), &json_body, Some(env!("API_PATH")));
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
                        match core::str::from_utf8(&buffer[..n]) {
                            Ok(s) => println!("Received response:\n---\n{}\n---", s),
                            Err(_) => println!("   Response not UTF-8 (binary data)"),
                        }
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
