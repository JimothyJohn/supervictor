// https://github.com/esp-rs/esp-hal/blob/main/examples/src/bin/wifi_embassy_dhcp.rs
//! Desktop version of the Supervictor application
//!
//! This version uses reqwest for HTTP requests and runs on desktop platforms
//!
//! Environment variables:
//!   HOST - Target host for HTTP requests

//% FEATURES: embassy esp-wifi esp-wifi/wifi esp-hal/unstable
//% CHIPS: esp32 esp32s2 esp32s3 esp32c2 esp32c3 esp32c6

use heapless::String as HString;
// use std::collections::HashMap;
use std::time::Duration;
use supervictor::models::UplinkMessage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load certificates and key
    let ca_cert = reqwest::Certificate::from_pem(include_bytes!("../../aws/AmazonRootCA1.pem"))?;

    // https://github.com/seanmonstar/reqwest/issues/2011#issuecomment-2801252988
    let client_cert = reqwest::Identity::from_pem(include_bytes!("../../aws/debian.pem"))?;

    // Build client with certificates
    let client = reqwest::Client::builder()
        .user_agent("Uplink/0.1.0 (Platform; ESP32-C3)")
        .use_rustls_tls()
        .add_root_certificate(ca_cert)
        .identity(client_cert)
        .build()?;

    // Get host from environment
    let host = env!("HOST");
    // let host = env::var("HOST").expect("HOST environment variable not set");

    // Create the message data
    let message = UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    };

    // Convert to JSON using serde-json-core
    let json_body: HString<512> =
        serde_json_core::to_string(&message).unwrap_or_else(|_| "{}".try_into().unwrap());

    println!("Starting message loop with 3 second interval");

    // Loop like in embedded version
    loop {
        // Make authenticated request
        match client.post(host).json(&json_body).send().await {
            Ok(response) => {
                // println!("Response status: {}", response.status());
                if let Ok(text) = response.text().await {
                    println!("Response body: {}", text);
                }
            }
            Err(e) => println!("Error sending request: {}", e),
        }

        // Wait before sending next message
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
