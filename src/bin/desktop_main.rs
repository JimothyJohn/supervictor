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
use std::{sync::Arc, time::Duration};

// AI-Generated comment: Imports for reading/writing to the TLS stream (async).
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream, // Tokio async TCP stream
};

// AI-Generated comment: Imports for Rustls TLS implementation.
use rustls::pki_types::ServerName;
use rustls_pemfile::{certs, private_key}; // To load PEM files
use supervictor::{
    models::uplink::UplinkMessage,
    network::http::{parse_response, post_request},
};
use tokio_rustls::TlsConnector; // Rustls connector for Tokio // For SNI

// AI-Generated comment: Standard web root CAs (optional but good practice).
// use webpki_roots::TLS_SERVER_ROOTS;

#[tokio::main]
async fn main() {
    // AI-Generated change: Handle the Result from socket_app
    if let Err(e) = socket_app().await {
        eprintln!("Application error: {}", e);
        std::process::exit(1); // Exit with error code if app fails
    }
    // No need for Ok(()) here as main doesn't return Result anymore
}

pub async fn socket_app() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // --- Network Setup ---
    let connector = match create_connector() {
        Ok(connector) => connector,
        Err(e) => {
            println!("Error creating connector: {}", e);
            return Err(e);
        }
    };

    // AI-Generated comment: Get base host from environment at compile time.
    let base_host = env!("HOST");
    // AI-Generated comment: Construct the host:port string for connection.
    let host_port = format!("{}:443", base_host); // Standard HTTPS port

    // AI-Generated comment: Resolve DNS asynchronously using Tokio.
    let socket_addr = tokio::net::lookup_host(&host_port)
        .await?
        .next()
        .ok_or("DNS resolution failed: No addresses found")?;

    // AI-Generated comment: Prepare the server name for SNI (Server Name Indication) and certificate validation.
    // AI-Generated comment: This MUST match the name on the server's certificate (supervictor.advin.io).
    let server_name = ServerName::try_from(base_host)?.to_owned();

    // --- Prepare Request Data ---

    let message = UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    };
    let json_body: HString<512> =
        serde_json_core::to_string(&message).unwrap_or_else(|_| "{}".try_into().unwrap());

    // --- Main Loop ---
    loop {
        match TcpStream::connect(socket_addr).await {
            Ok(socket) => {
                match connector.connect(server_name.clone(), socket).await {
                    Ok(mut tls_stream) => {
                        // AI-Generated comment: Manually construct the HTTP POST request.
                        let request = post_request(base_host, &json_body, Some(env!("API_PATH")));

                        if let Err(e) = tls_stream.write_all(request.as_bytes()).await {
                            println!("Error writing request: {}", e);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue; // Try reconnecting
                        }

                        // AI-Generated comment: Read the response (async).
                        let mut response_buf = Vec::new();
                        match tls_stream.read_to_end(&mut response_buf).await {
                            Ok(bytes_read) => {
                                // AI-Generated comment: Attempt to print response as UTF-8.
                                match String::from_utf8(response_buf) {
                                    Ok(response_str) => match parse_response(&response_str) {
                                        Ok(response) => {
                                            println!(
                                                "Parsed response:\n---\n{}\n---",
                                                serde_json_core::to_string::<_, 1024>(
                                                    &response.body
                                                )
                                                .unwrap()
                                            )
                                        }
                                        Err(e) => println!("Error parsing response: {}", e),
                                    },
                                    Err(_) => println!(
                                        "Received non-UTF8 response ({} bytes)",
                                        bytes_read
                                    ),
                                }
                            }
                            Err(e) => println!("Error reading response: {}", e),
                        }
                    }
                    Err(e) => println!("TLS handshake error: {}", e),
                }
            }
            Err(e) => println!("TCP connection error: {}", e),
        }

        // Wait before next attempt
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub fn create_connector() -> Result<TlsConnector, Box<dyn std::error::Error>> {
    // AI-Generated comment: Load the Amazon CA certificate (server validation).
    let mut ca_reader = concat!(include_str!("../../certs/AmazonRootCA1.pem"), "\0").as_bytes();

    // std::io::BufReader::new(&include_bytes!("../../certs/AmazonRootCA1.pem")[..]);
    // AI-Generated comment: rustls_pemfile::certs returns an iterator, collect results before using ?
    let ca_certs = certs(&mut ca_reader).collect::<Result<Vec<_>, _>>()?;
    if ca_certs.is_empty() {
        return Err("Could not load CA certificate".into());
    }

    // AI-Generated comment: Load the client certificate.
    let mut client_cert_reader =
        std::io::BufReader::new(&include_bytes!("../../certs/temp-250423.crt")[..]);
    // AI-Generated comment: Collect results from iterator before using ?
    let client_certs = certs(&mut client_cert_reader).collect::<Result<Vec<_>, _>>()?;
    if client_certs.is_empty() {
        return Err("Could not load client certificate".into());
    }

    // AI-Generated comment: Load the client private key.
    let mut client_key_reader =
        std::io::BufReader::new(&include_bytes!("../../certs/temp-250423.key")[..]);
    // AI-Generated comment: private_key returns the first valid key found.
    let client_key_parsed = private_key(&mut client_key_reader)?;
    let client_key = match client_key_parsed {
        Some(key) => key,
        None => return Err("Could not load private key".into()),
    };

    // AI-Generated comment: Create a Rustls root certificate store.
    let mut root_cert_store = rustls::RootCertStore::empty();
    // AI-Generated comment: Add standard web CAs (optional, good practice for general TLS).
    // root_cert_store.extend(TLS_SERVER_ROOTS.iter().cloned());
    // AI-Generated comment: Add our specific Amazon CA.
    root_cert_store.add_parsable_certificates(ca_certs);

    // AI-Generated comment: Create Rustls client configuration.
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_cert_store) // Trust the CA(s)
        .with_client_auth_cert(client_certs, client_key)?; // Provide client cert and key

    // AI-Generated comment: Create a Tokio TlsConnector (wraps the Rustls config).
    let connector = TlsConnector::from(Arc::new(config));
    Ok(connector)
}

/*
// Higher-level version of above method
async fn reqwest_app() -> Result<(), Box<dyn std::error::Error>> {
    // Load certificates and key
    let ca_cert = reqwest::Certificate::from_pem(include_bytes!("../../certs/AmazonRootCA1.pem"))?;

    // https://github.com/seanmonstar/reqwest/issues/2011#issuecomment-2801252988
    let client_cert = reqwest::Identity::from_pem(include_bytes!("../../certs/temp-250423.pem"))?;

    // Build client with certificates
    let client = reqwest::Client::builder()
        .user_agent("Uplink/0.1.0 (Platform; ESP32-C3)")
        .use_rustls_tls()
        .add_root_certificate(ca_cert)
        .identity(client_cert)
        .build()?;

    // Get host from environment
    let base_host = env!("HOST");
    // let host = env::var("HOST").expect("HOST environment variable not set");

    // AI-Generated comment: Construct the full target URL by prepending scheme and appending path.
    let target_url = format!("https://{}{}", base_host, env!("API_PATH"));

    // Create the message data
    let message = UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    };

    // Convert to JSON using serde-json-core
    let json_body: HString<512> =
        serde_json_core::to_string(&message).unwrap_or_else(|_| "{}".try_into().unwrap());

    // Loop like in embedded version
    loop {
        // Make authenticated request
        match client.post(&target_url).json(&json_body).send().await {
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
*/
