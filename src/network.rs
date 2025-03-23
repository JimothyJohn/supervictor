use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Runner, Stack,
};
use embassy_time::{Duration, Timer};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};

use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    headers::ContentType,
    request::{Method, RequestBuilder},
};

use crate::constants::endpoints::USER_AGENT;

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

// https://esp32.implrust.com/wifi/embassy/http-request.html
// TODO add TLS Verify once able
pub async fn access_website<'a>(stack: &'a Stack<'a>, tls_seed: u64) {
    // Message buffers
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut response_buffer = [0u8; 1024];
    let mut body_buffer = [0u8; 1024];

    // Borrowed variables
    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(*stack, &tcp_state);
    let dns = DnsSocket::new(*stack);

    // Create TLS client
    let mut client = HttpClient::new_with_tls(
        &tcp,
        &dns,
        TlsConfig::new(tls_seed, &mut rx_buffer, &mut tx_buffer, TlsVerify::None),
    );

    // Create the request handler
    let request_builder = match client.request(Method::POST, env!("HOST")).await {
        Ok(builder) => builder,
        Err(e) => {
            // AI-generated: Properly handle request creation errors
            println!("Error creating HTTP request: {:?}", e);
            return; // Exit the function early since we can't proceed
        }
    };

    // Create the request
    let mut request = request_builder
        .headers(&[("User-Agent", USER_AGENT), ("Accept", "application/json")])
        .content_type(ContentType::ApplicationJson)
        .body(&b"{\"message\":\"PINGS\"}"[..]);

    // Send the request and get a response
    let response = match request.send(&mut response_buffer).await {
        Ok(res) => res,
        Err(e) => {
            println!("Error in HTTP request: {:?}", e);
            return;
        }
    };

    // Read the response body into body_buffer
    match response.body().reader().read_to_end(&mut body_buffer).await {
        Ok(res) => res,
        Err(e) => {
            println!("Error reading response body: {:?}", e);
            return;
        }
    };

    // Convert the response body to a string
    match core::str::from_utf8(&body_buffer) {
        Ok(body_str) => {
            // Successfully converted bytes to UTF-8 string
            println!("Response body:\n\n{}", body_str);
        }
        Err(e) => {
            // Failed to convert to UTF-8 string
            println!("Error: Response contains invalid UTF-8: {:?}", e);
        }
    }
}
