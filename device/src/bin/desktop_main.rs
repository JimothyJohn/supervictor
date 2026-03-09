//! Desktop version of the Supervictor application
//!
//! This version uses rustls for TLS and runs on desktop platforms.
//!
//! Environment variables (all runtime):
//!   HOST        - Target host for HTTP requests (e.g. supervictor.advin.io)
//!   CA_PEM      - Path to CA certificate PEM for server verification
//!   CLIENT_CERT - Path to client certificate PEM for mTLS
//!   CLIENT_KEY  - Path to client private key PEM for mTLS

use heapless::String as HString;
use std::{env, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use rustls::pki_types::ServerName;
use rustls_pemfile::{certs, private_key};
use supervictor::{
    models::uplink::UplinkMessage,
    network::http::{parse_response, post_request},
};
use tokio_rustls::TlsConnector;

#[tokio::main]
async fn main() {
    if let Err(e) = socket_app().await {
        eprintln!("Application error: {}", e);
        std::process::exit(1);
    }
}

pub async fn socket_app() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let connector = create_connector()?;

    let base_host = env::var("HOST").expect("HOST environment variable not set");
    let host_port = format!("{}:443", base_host);

    let socket_addr = tokio::net::lookup_host(&host_port)
        .await?
        .next()
        .ok_or("DNS resolution failed: No addresses found")?;

    // SNI server name must match the server's certificate
    let server_name = ServerName::try_from(base_host.as_str())?.to_owned();

    let message = UplinkMessage {
        id: "1234567890".try_into().unwrap(),
        current: 100,
    };
    let json_body: HString<512> =
        serde_json_core::to_string(&message).unwrap_or_else(|_| "{}".try_into().unwrap());

    loop {
        match TcpStream::connect(socket_addr).await {
            Ok(socket) => match connector.connect(server_name.clone(), socket).await {
                Ok(mut tls_stream) => {
                    let request = post_request(&base_host, &json_body, None);

                    if let Err(e) = tls_stream.write_all(request.as_bytes()).await {
                        println!("Error writing request: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }

                    let mut response_buf = Vec::new();
                    match tls_stream.read_to_end(&mut response_buf).await {
                        Ok(bytes_read) => match String::from_utf8(response_buf) {
                            Ok(response_str) => match parse_response(&response_str) {
                                Ok(response) => {
                                    println!(
                                        "Parsed response:\n---\n{}\n---",
                                        serde_json_core::to_string::<_, 1024>(&response.body)
                                            .unwrap()
                                    )
                                }
                                Err(e) => println!("Error parsing response: {}", e),
                            },
                            Err(_) => {
                                println!("Received non-UTF8 response ({} bytes)", bytes_read)
                            }
                        },
                        Err(e) => println!("Error reading response: {}", e),
                    }
                }
                Err(e) => println!("TLS handshake error: {}", e),
            },
            Err(e) => println!("TCP connection error: {}", e),
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn env_or(var: &str, default: &str) -> String {
    env::var(var).unwrap_or_else(|_| default.to_string())
}

pub fn create_connector() -> Result<TlsConnector, Box<dyn std::error::Error>> {
    let ca_path = env_or("CA_PEM", "../certs/ca/AmazonRootCA1.pem");
    let cert_path = env_or("CLIENT_CERT", "../certs/devices/test-device/client.pem");
    let key_path = env_or("CLIENT_KEY", "../certs/devices/test-device/client.key");

    // Load CA certificate (server verification)
    let ca_pem = std::fs::read_to_string(&ca_path)
        .map_err(|e| format!("Failed to read CA cert at {}: {}", ca_path, e))?;
    let mut ca_reader = ca_pem.as_bytes();
    let ca_certs = certs(&mut ca_reader).collect::<Result<Vec<_>, _>>()?;
    if ca_certs.is_empty() {
        return Err(format!("No certificates found in {}", ca_path).into());
    }

    // Load client certificate (mTLS identity)
    let cert_pem = std::fs::read(&cert_path)
        .map_err(|e| format!("Failed to read client cert at {}: {}", cert_path, e))?;
    let mut cert_reader = std::io::BufReader::new(&cert_pem[..]);
    let client_certs = certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;
    if client_certs.is_empty() {
        return Err(format!("No certificates found in {}", cert_path).into());
    }

    // Load client private key
    let key_pem = std::fs::read(&key_path)
        .map_err(|e| format!("Failed to read client key at {}: {}", key_path, e))?;
    let mut key_reader = std::io::BufReader::new(&key_pem[..]);
    let client_key = private_key(&mut key_reader)?
        .ok_or_else(|| format!("No private key found in {}", key_path))?;

    let mut root_cert_store = rustls::RootCertStore::empty();
    root_cert_store.add_parsable_certificates(ca_certs);

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_client_auth_cert(client_certs, client_key)?;

    Ok(TlsConnector::from(Arc::new(config)))
}
