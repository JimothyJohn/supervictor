use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};
use heapless::String as HString;
use serde::Serialize;
use serde_json_core::to_string;

use crate::constants::endpoints::USER_AGENT;
use crate::models::RequestData;

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    // println!("Device capabilities: {:?}", controller.capabilities());

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
            // Convert length to string and append
            let len = json.len();
            let mut len_str = HString::<16>::new();
            // Simple conversion of number to string
            let mut n = len;
            if n == 0 {
                len_str.push_str("0").unwrap();
            } else {
                let mut digits = HString::<16>::new();
                while n > 0 {
                    let digit = (n % 10) as u8 + b'0';
                    digits.push(digit as char).unwrap();
                    n /= 10;
                }
                // Reverse the digits
                for i in (0..digits.len()).rev() {
                    len_str.push(digits.as_bytes()[i] as char).unwrap();
                }
            }
            request.push_str(&len_str).unwrap();
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
