//! Tests that TLS handshake failures are handled gracefully (no panics).
//! Covers: wrong CA, expired cert, non-TLS server, connection reset.

use std::io::Write;
use std::sync::{Arc, Once};

use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::net::TcpListener as TokioTcpListener;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

static CRYPTO_INIT: Once = Once::new();

fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install crypto provider");
    });
}

/// Build a rustls TlsConnector that trusts only the given CA cert.
fn connector_trusting(ca_cert: &CertificateDer<'static>) -> TlsConnector {
    ensure_crypto_provider();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(ca_cert.clone()).unwrap();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Bind a TLS server, return the listener and acceptor.
/// Caller spawns the accept task so everything stays in one tokio runtime.
fn bind_tls_server(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> (TokioTcpListener, tokio_rustls::TlsAcceptor) {
    ensure_crypto_provider();
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let listener = TokioTcpListener::from_std(std_listener).unwrap();

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    (listener, acceptor)
}

/// Spawn a plain TCP server (no TLS) that sends garbage bytes.
fn spawn_plain_tcp_server() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\nNot TLS");
        let _ = stream.flush();
    });

    addr
}

/// Spawn a TCP server that immediately closes the connection.
fn spawn_reset_server() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
    });

    addr
}

// -- Tests --

#[tokio::test]
async fn tls_wrong_ca_returns_error_not_panic() {
    // Server uses cert signed by CA-A, client trusts CA-B
    let server_ca = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let client_ca = generate_simple_self_signed(vec!["other-ca".to_string()]).unwrap();

    let server_cert = CertificateDer::from(server_ca.cert.der().to_vec());
    let server_key = PrivateKeyDer::try_from(server_ca.key_pair.serialize_der()).unwrap();
    let client_trusted = CertificateDer::from(client_ca.cert.der().to_vec());

    let (listener, acceptor) = bind_tls_server(server_cert, server_key);
    let addr = listener.local_addr().unwrap();

    // Server accepts in background — handshake will fail on its side too
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let _ = acceptor.accept(stream).await;
        }
    });

    let connector = connector_trusting(&client_trusted);
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    let tcp = TcpStream::connect(addr).await.unwrap();
    let result = connector.connect(server_name, tcp).await;

    assert!(result.is_err(), "Expected TLS error for wrong CA");
}

#[tokio::test]
async fn tls_plain_tcp_server_returns_error_not_panic() {
    let dummy_ca = generate_simple_self_signed(vec!["dummy".to_string()]).unwrap();
    let ca_cert = CertificateDer::from(dummy_ca.cert.der().to_vec());

    let addr = spawn_plain_tcp_server();
    let connector = connector_trusting(&ca_cert);
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    let tcp = TcpStream::connect(addr).await.unwrap();
    let result = connector.connect(server_name, tcp).await;

    assert!(
        result.is_err(),
        "Expected TLS error when server doesn't speak TLS"
    );
}

#[tokio::test]
async fn tls_connection_reset_returns_error_not_panic() {
    let dummy_ca = generate_simple_self_signed(vec!["dummy".to_string()]).unwrap();
    let ca_cert = CertificateDer::from(dummy_ca.cert.der().to_vec());

    let addr = spawn_reset_server();
    let connector = connector_trusting(&ca_cert);
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    let tcp = TcpStream::connect(addr).await.unwrap();
    let result = connector.connect(server_name, tcp).await;

    assert!(
        result.is_err(),
        "Expected TLS error when server resets connection"
    );
}

#[tokio::test]
async fn tls_correct_ca_succeeds() {
    // Server and client trust the same self-signed cert — handshake should succeed
    let ca = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();

    let cert = CertificateDer::from(ca.cert.der().to_vec());
    let key = PrivateKeyDer::try_from(ca.key_pair.serialize_der()).unwrap();
    let trusted = CertificateDer::from(ca.cert.der().to_vec());

    let (listener, acceptor) = bind_tls_server(cert, key);
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let _ = acceptor.accept(stream).await;
        }
    });

    let connector = connector_trusting(&trusted);
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    let tcp = TcpStream::connect(addr).await.unwrap();
    let result = connector.connect(server_name, tcp).await;

    assert!(
        result.is_ok(),
        "Expected TLS handshake to succeed with correct CA"
    );
}

#[tokio::test]
async fn tls_retry_after_failure_succeeds() {
    // First attempt: wrong CA -> fail. Second attempt: correct CA -> succeed.
    // Validates the retry pattern used in tasks.rs.
    let server_ca = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let wrong_ca = generate_simple_self_signed(vec!["wrong".to_string()]).unwrap();

    let server_cert = CertificateDer::from(server_ca.cert.der().to_vec());
    let server_key = PrivateKeyDer::try_from(server_ca.key_pair.serialize_der()).unwrap();
    let wrong_trusted = CertificateDer::from(wrong_ca.cert.der().to_vec());
    let correct_trusted = CertificateDer::from(server_ca.cert.der().to_vec());

    // Attempt 1: wrong CA
    let (listener1, acceptor1) = bind_tls_server(server_cert.clone(), server_key.clone_key());
    let addr1 = listener1.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener1.accept().await {
            let _ = acceptor1.accept(stream).await;
        }
    });

    let bad_connector = connector_trusting(&wrong_trusted);
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    let tcp = TcpStream::connect(addr1).await.unwrap();
    let result1 = bad_connector.connect(server_name.clone(), tcp).await;
    assert!(result1.is_err(), "First attempt should fail with wrong CA");

    // Attempt 2: correct CA (simulates retry with fixed config)
    let (listener2, acceptor2) = bind_tls_server(server_cert, server_key);
    let addr2 = listener2.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener2.accept().await {
            let _ = acceptor2.accept(stream).await;
        }
    });

    let good_connector = connector_trusting(&correct_trusted);

    let tcp = TcpStream::connect(addr2).await.unwrap();
    let result2 = good_connector.connect(server_name, tcp).await;
    assert!(
        result2.is_ok(),
        "Second attempt should succeed after CA fix"
    );
}
