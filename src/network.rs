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

use heapless::String as HString;
use reqwless::client::{HttpClient, TlsConfig};
use serde::Serialize;

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

// https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/GET
pub fn get_request(host: &str, path: Option<&str>) -> heapless::String<128> {
    let mut request = heapless::String::<128>::new();
    // Use the provided path or default to "/"
    let path = path.unwrap_or("/");

    // Use the path in the request line instead of hardcoded "/"
    request.push_str("GET ").unwrap();
    request.push_str(path).unwrap();
    request.push_str(" HTTP/1.0").unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("Host: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("User-Agent: ").unwrap();
    request.push_str(USER_AGENT).unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("Accept: */*").unwrap();
    request.push_str("\r\n\r\n").unwrap();
    request
}

/// Create an HTTP POST request with JSON body
pub fn post_request<T>(host: &str, data: &T, path: Option<&str>) -> HString<512>
where
    T: Serialize,
{
    let mut request = HString::<512>::new();

    // Format the request path
    let endpoint = path.unwrap_or("/");

    // Start building the request
    request.push_str("POST ").unwrap();
    request.push_str(endpoint).unwrap();
    request.push_str(" HTTP/1.1\r\n").unwrap();
    request.push_str("Host: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request
        .push_str("Content-Type: application/json\r\n")
        .unwrap();

    // Serialize the data to JSON
    let json_result = serde_json_core::to_string::<T, 256>(data);

    match json_result {
        Ok(json) => {
            // Add Content-Length header
            request.push_str("Content-Length: ").unwrap();

            // Convert length to string - simplified approach
            let len = json.len();
            // For most HTTP requests, content length will be small
            // This handles up to 5 digits (lengths up to 99999)
            let mut buffer = [0u8; 5];
            let mut i = 0;

            // Handle zero case
            if len == 0 {
                request.push_str("0").unwrap();
            } else {
                // Convert number to digits
                let mut n = len;
                while n > 0 {
                    buffer[i] = (n % 10) as u8 + b'0';
                    n /= 10;
                    i += 1;
                }

                // Add digits in reverse order
                while i > 0 {
                    i -= 1;
                    request.push(buffer[i] as char).unwrap();
                }
            }

            request.push_str("\r\n\r\n").unwrap();

            // Add the JSON body
            request.push_str(&json).unwrap();
        }
        Err(_) => {
            // Handle serialization error
            request.push_str("Content-Length: 0\r\n\r\n").unwrap();
        }
    }

    request
}

// https://esp32.implrust.com/wifi/embassy/http-request.html
pub async fn access_website<'a>(stack: &'a Stack<'a>, tls_seed: u64) {
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let dns = DnsSocket::new(*stack);
    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(*stack, &tcp_state);

    let tls = TlsConfig::new(
        tls_seed,
        &mut rx_buffer,
        &mut tx_buffer,
        reqwless::client::TlsVerify::None,
    );

    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
    let mut buffer = [0u8; 4096];
    // TODO JSON https://docs.rs/reqwest/latest/reqwest/#json
    let mut http_req = client
        .request(
            reqwless::request::Method::GET,
            "https://jsonplaceholder.typicode.com/posts/1",
        )
        .await
        .unwrap();
    let response = http_req.send(&mut buffer).await.unwrap();

    println!("Got response");
    let res = response.body().read_to_end().await.unwrap();

    let content = core::str::from_utf8(res).unwrap();
    println!("{}", content);
}
