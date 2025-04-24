use core::ffi::CStr;

use crate::config::*;
use crate::models::UplinkMessage;
use crate::network::{http::post_request, tls::load_certificates};
use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, Stack};
use embassy_time::{Duration, Timer};
use esp_mbedtls::{asynch::Session, Mode, Tls, TlsVersion};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};
use heapless::String as HString;

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

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

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

    let address = match stack
        .dns_query(env!("HOST"), embassy_net::dns::DnsQueryType::A)
        .await
    {
        Ok(addresses) => {
            if let Some(first_addr) = addresses.first() {
                *first_addr
            } else {
                println!("No addresses returned from DNS query");
                panic!("No addresses returned from DNS query");
            }
        }
        Err(e) => {
            println!("DNS resolution failed: {:?}", e);
            panic!("DNS resolution failed: {:?}", e);
        }
    };

    let remote_endpoint = (address, AWS_IOT_PORT);

    let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
    let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    socket.set_timeout(Some(SOCKET_TIMEOUT));

    println!("Connecting...");
    let r = socket.connect(remote_endpoint).await;

    if let Err(e) = r {
        println!("connect error: {:?}", e);
        #[allow(clippy::empty_loop)]
        loop {}
    }

    // AI-Generated comment: Create the CStr for the servername *before* Session::new.
    // This ensures the CStr reference lives long enough for the Session::new call.
    let host_cstr = match CStr::from_bytes_with_nul(concat!(env!("HOST"), "\0").as_bytes()) {
        Ok(cstr) => cstr,
        Err(e) => {
            // AI-Generated comment: Log and panic if HOST env var is invalid (contains null bytes).
            println!("   ❌ FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
            panic!("FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
        }
    };

    let certs = load_certificates();

    // AI-Generated comment: Initialize the TLS session, passing the pre-validated host_cstr.
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

    loop {
        // Try sending a simple HTTP request to verify the connection
        let request = post_request(env!("HOST"), &json_body, Some("/supervictor"));
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
                        Ok(s) => println!("   Response: {}", s),
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
