// https://github.com/esp-rs/esp-hal/blob/main/examples/src/bin/wifi_embassy_dhcp.rs
//! Desktop version of the Supervictor application
//!
//! This version uses reqwest for HTTP requests and runs on desktop platforms
//!
//! Environment variables:
//!   HOST - Target host for HTTP requests

//% FEATURES: embassy esp-wifi esp-wifi/wifi esp-hal/unstable
//% CHIPS: esp32 esp32s2 esp32s3 esp32c2 esp32c3 esp32c6

use std::collections::HashMap;
// use supervictor::models::EchoResponse;

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

    // Make authenticated request
    let resp = client
        .post(env!("HOST"))
        .body("the exact body that is sent")
        .send()
        .await?
        .json::<HashMap<String, String>>()
        .await?;

    println!("{resp:#?}");
    Ok(())
}
