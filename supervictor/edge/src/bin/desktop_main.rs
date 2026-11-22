//! Desktop version of the Supervictor application
//!
//! This version uses rustls for TLS and runs on desktop platforms (and the
//! bench Raspberry Pi). Each tick it posts one simulated telemetry sample —
//! same simulator as the firmware, so a Pi looks like any other board on
//! the dashboard.
//!
//! Environment variables (all runtime):
//!   HOST        - Target host for HTTP requests (e.g. supervictor.advin.io)
//!   PORT        - TLS port on that host (default 443; a local Quickstart
//!                 endpoint listens on 8443)
//!   API_PATH    - Request path: "/" (default), or a prefix when something in
//!                 front mounts the endpoint under one
//!   DEVICE_ID   - Device id to post as (default "1234567890"); should match
//!                 the client cert's CN once the device is registered
//!   INTERVAL_S  - Seconds between uplinks (default 5; jittered ±20 %)
//!   BURST       - Samples per uplink, sent as an 8-bit `levels` burst
//!                 (default 50 → one sample every 100 ms; 1 disables;
//!                 capped at 50 per uplink)
//!   SIM_SEED    - u64 seed salt for reproducible telemetry (default: clock)
//!   SIM_PROFILE - Machine to impersonate: generic (default), extruder,
//!                 press-brake, cnc, saw, oven, compressor, conveyor, molder,
//!                 welder, pump (see models::profile)
//!   SIM_SPEED   - Simulated seconds per real second for the machine model
//!                 (default 1; e.g. 60 compresses a working day into minutes)
//!   CA_PEM      - Path to CA certificate PEM for server verification
//!   CLIENT_CERT - Path to client certificate PEM for mTLS
//!   CLIENT_KEY  - Path to client private key PEM for mTLS

use heapless::String as HString;
use std::{
    env,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use rustls::pki_types::ServerName;
use rustls_pemfile::{certs, private_key};
use supervictor::{
    models::profile::Profile,
    models::sim::{seed_from, Hardware, Simulator},
    network::http::{parse_response, post_request},
};
use tokio_rustls::TlsConnector;

/// How long one uplink response may take, head to last body byte.
const RESPONSE_DEADLINE: Duration = Duration::from_secs(10);
/// Largest uplink response, head and body, the client will buffer.
const MAX_RESPONSE: usize = 64 * 1024;

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
    let port: u16 = env_or("PORT", "443")
        .parse()
        .map_err(|_| "PORT must be a TCP port number")?;
    let host_port = format!("{}:{}", base_host, port);

    // Every address the name resolves to: `localhost` is usually both ::1
    // and 127.0.0.1, and an endpoint bound to 0.0.0.0 answers only the
    // latter, so each tick tries them in order until one connects.
    let socket_addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host(&host_port).await?.collect();
    if socket_addrs.is_empty() {
        return Err("DNS resolution failed: No addresses found".into());
    }

    // SNI server name must match the server's certificate
    let server_name = ServerName::try_from(base_host.as_str())?.to_owned();

    // API_PATH: "/" for the endpoint itself, a prefix when something in front
    // mounts it under one. Runtime here; the embedded build bakes it in at
    // compile time.
    let api_path = env_or("API_PATH", "/");
    let device_id: HString<64> = env_or("DEVICE_ID", "1234567890")
        .as_str()
        .try_into()
        .map_err(|_| "DEVICE_ID longer than 64 bytes")?;
    let interval_s: u32 = env_or("INTERVAL_S", "5")
        .parse()
        .map_err(|_| "INTERVAL_S must be a positive integer")?;
    if interval_s == 0 {
        return Err("INTERVAL_S must be at least 1".into());
    }
    let burst: usize = env_or("BURST", "50")
        .parse()
        .map_err(|_| "BURST must be a positive integer")?;
    if burst == 0 {
        return Err("BURST must be at least 1".into());
    }
    let salt: u64 = match env::var("SIM_SEED") {
        Ok(s) => s.parse().map_err(|_| "SIM_SEED must be a u64")?,
        Err(_) => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
    };
    let profile_name = env_or("SIM_PROFILE", "generic");
    let profile = Profile::parse(&profile_name)
        .ok_or_else(|| format!("SIM_PROFILE '{profile_name}' is not a known machine profile"))?;
    let speed: u32 = env_or("SIM_SPEED", "1")
        .parse()
        .map_err(|_| "SIM_SPEED must be a positive integer")?;
    if speed == 0 {
        return Err("SIM_SPEED must be at least 1".into());
    }
    let mut sim = Simulator::with_profile(seed_from(device_id.as_str(), salt), profile);
    sim.set_time_scale(speed);
    println!(
        "{} posting to {} every {}s as a {} (x{} time)",
        device_id,
        base_host,
        interval_s,
        profile.name(),
        speed
    );
    let booted = Instant::now();

    loop {
        // post_request serializes its payload itself; handing it a pre-serialized
        // string double-encoded the body as a JSON string literal.
        let uptime_s = u32::try_from(booted.elapsed().as_secs()).unwrap_or(u32::MAX);
        let message = sim.uplink(
            device_id.clone(),
            uptime_s,
            interval_s,
            burst,
            Hardware::default(),
        );
        let delay = Duration::from_millis(u64::from(sim.next_delay_ms(interval_s * 1_000)));

        match connect_any(&socket_addrs).await {
            Ok(socket) => match connector.connect(server_name.clone(), socket).await {
                Ok(mut tls_stream) => {
                    let request = match post_request(&base_host, &message, Some(&api_path)) {
                        Ok(r) => r,
                        Err(e) => {
                            println!("Error building request: {}", e);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };

                    if let Err(e) = tls_stream.write_all(request.as_bytes()).await {
                        println!("Error writing request: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }

                    match read_response(&mut tls_stream, RESPONSE_DEADLINE).await {
                        Ok(response_buf) => match String::from_utf8(response_buf) {
                            Ok(response_str) => match parse_response(&response_str) {
                                Ok(response) => {
                                    println!(
                                        "seq {} current {} ({} samples) -> {}",
                                        message.seq.unwrap_or(0),
                                        message.current,
                                        (message.levels.len() / supervictor_common::levels::COUNT)
                                            .max(1),
                                        response.body
                                    )
                                }
                                Err(e) => println!("Error parsing response: {}", e),
                            },
                            Err(e) => {
                                println!(
                                    "Received non-UTF8 response ({} bytes)",
                                    e.as_bytes().len()
                                )
                            }
                        },
                        Err(e) => println!("Error reading response: {}", e),
                    }
                }
                Err(e) => println!("TLS handshake error: {}", e),
            },
            Err(e) => println!("TCP connection error: {}", e),
        }

        tokio::time::sleep(delay).await;
    }
}

/// Connect to the first address that accepts; the last error is the one
/// reported when none does.
async fn connect_any(addrs: &[std::net::SocketAddr]) -> std::io::Result<TcpStream> {
    let mut last = std::io::Error::other("no addresses to connect to");
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(socket) => return Ok(socket),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Read one HTTP response from the stream: the head, then the body to its
/// `Content-Length`, or to EOF when it has none. The uplink POST asks for
/// keep-alive (the firmware reuses its TLS session), so the endpoint holds
/// the connection open after answering; reading to EOF instead hung the
/// client after its first uplink. The whole read is bounded by `deadline`
/// and by `MAX_RESPONSE`.
async fn read_response<S>(stream: &mut S, deadline: Duration) -> std::io::Result<Vec<u8>>
where
    S: tokio::io::AsyncRead + Unpin,
{
    use std::io::{Error, ErrorKind};
    use supervictor::ota::http::{parse_head, HeadError};

    let read = async {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        // Total length once the head is in: Some(n) when framed, None to EOF.
        let mut framed: Option<Option<usize>> = None;
        loop {
            if framed.is_none() {
                match parse_head(&buf) {
                    Ok((head, body_start)) => {
                        let total = head
                            .content_length
                            .map(|n| body_start.saturating_add(n as usize));
                        if total.is_some_and(|t| t > MAX_RESPONSE) {
                            return Err(Error::new(
                                ErrorKind::InvalidData,
                                "response larger than the client buffers",
                            ));
                        }
                        framed = Some(total);
                    }
                    Err(HeadError::Incomplete) => {}
                    Err(e) => {
                        return Err(Error::new(
                            ErrorKind::InvalidData,
                            format!("bad response head: {e:?}"),
                        ))
                    }
                }
            }
            if let Some(Some(total)) = framed {
                if buf.len() >= total {
                    buf.truncate(total);
                    return Ok(buf);
                }
            }
            if buf.len() > MAX_RESPONSE {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "response larger than the client buffers",
                ));
            }
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return match framed {
                    Some(None) => Ok(buf),
                    _ => Err(Error::new(
                        ErrorKind::UnexpectedEof,
                        "connection closed before the response was complete",
                    )),
                };
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    };
    tokio::time::timeout(deadline, read)
        .await
        .map_err(|_| Error::new(ErrorKind::TimedOut, "no complete response in time"))?
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt, DuplexStream};

    const DEADLINE: Duration = Duration::from_secs(10);
    const ACK: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
content-length: 11\r\n\r\n{\"ok\":true}";

    /// A peer that writes `chunks` (yielding between them) and then keeps
    /// its end open: the client sees no EOF while the handle lives.
    fn peer(chunks: Vec<Vec<u8>>) -> (DuplexStream, tokio::task::JoinHandle<DuplexStream>) {
        let (client, mut server) = duplex(1 << 20);
        let handle = tokio::spawn(async move {
            for c in chunks {
                server.write_all(&c).await.unwrap();
                tokio::task::yield_now().await;
            }
            server
        });
        (client, handle)
    }

    /// A peer that writes `wire` and closes.
    async fn closing_peer(wire: &[u8]) -> DuplexStream {
        let (client, mut server) = duplex(1 << 20);
        server.write_all(wire).await.unwrap();
        drop(server);
        client
    }

    async fn bounded(client: &mut DuplexStream) -> std::io::Result<Vec<u8>> {
        tokio::time::timeout(Duration::from_secs(60), read_response(client, DEADLINE))
            .await
            .expect("read_response outlived its deadline")
    }

    /// The regression: the endpoint answers the keep-alive POST and holds
    /// the connection open. The response is complete at Content-Length; a
    /// client that waits for EOF never returns and the sim posts once.
    #[tokio::test(start_paused = true)]
    async fn keep_alive_response_completes_at_content_length() {
        let (mut client, _server) = peer(vec![ACK.to_vec()]);
        assert_eq!(bounded(&mut client).await.unwrap(), ACK);
    }

    #[tokio::test(start_paused = true)]
    async fn head_and_body_split_across_reads() {
        let (head, body) = ACK.split_at(20);
        let (b1, b2) = body.split_at(body.len() - 3);
        let (mut client, _server) = peer(vec![head.to_vec(), b1.to_vec(), b2.to_vec()]);
        assert_eq!(bounded(&mut client).await.unwrap(), ACK);
    }

    /// Bytes past the framed length are not part of this response.
    #[tokio::test(start_paused = true)]
    async fn trailing_bytes_past_content_length_are_dropped() {
        let mut wire = ACK.to_vec();
        wire.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
        let (mut client, _server) = peer(vec![wire]);
        assert_eq!(bounded(&mut client).await.unwrap(), ACK);
    }

    /// No Content-Length: the body runs to EOF, as with Connection: close.
    #[tokio::test(start_paused = true)]
    async fn unframed_body_reads_to_eof() {
        let wire = b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\nhello";
        let mut client = closing_peer(wire).await;
        assert_eq!(bounded(&mut client).await.unwrap(), wire);
    }

    /// A peer that accepts and says nothing costs the deadline, not forever.
    #[tokio::test(start_paused = true)]
    async fn silent_peer_times_out_at_the_deadline() {
        let (mut client, _server) = peer(vec![]);
        let started = tokio::time::Instant::now();
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() <= DEADLINE + Duration::from_millis(1));
    }

    /// A body shorter than its Content-Length, then held open: timeout.
    #[tokio::test(start_paused = true)]
    async fn short_body_held_open_times_out() {
        let (mut client, _server) = peer(vec![ACK[..ACK.len() - 1].to_vec()]);
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }

    /// A body cut short by EOF is an error, not a partial success.
    #[tokio::test(start_paused = true)]
    async fn short_body_then_eof_is_an_error() {
        let mut client = closing_peer(&ACK[..ACK.len() - 1]).await;
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test(start_paused = true)]
    async fn eof_before_any_byte_is_an_error() {
        let mut client = closing_peer(b"").await;
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_head_is_an_error() {
        let (mut client, _server) = peer(vec![b"SSH-2.0-OpenSSH_9.6\r\n\r\n".to_vec()]);
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    /// A peer that never ends its head cannot grow the buffer without bound.
    #[tokio::test(start_paused = true)]
    async fn endless_head_is_capped() {
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        while wire.len() <= MAX_RESPONSE + 1024 {
            wire.extend_from_slice(b"x-pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
        }
        let (mut client, _server) = peer(vec![wire]);
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    /// A Content-Length past the cap is refused before reading the body.
    #[tokio::test(start_paused = true)]
    async fn oversized_content_length_is_refused() {
        let wire = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n",
            MAX_RESPONSE + 1
        );
        let (mut client, _server) = peer(vec![wire.into_bytes()]);
        let err = bounded(&mut client).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
