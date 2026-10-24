//! In-binary mTLS termination (feature `tls`).
//!
//! The single-binary deployment path: no Caddy, no API Gateway in front.
//! The binary listens TLS itself, requires and verifies a client
//! certificate against `TLS_CLIENT_CA`, and hands the peer certificate's
//! subject DN to every request as [`ConnectInfo<TlsPeer>`]. In this mode
//! the `x-ssl-client-subject-dn` / `x-amzn-request-context` headers are
//! ignored entirely — the identity comes from the handshake, and a client
//! cannot forge what it did not prove.
//!
//! Handshakes run on their own tasks: a client that connects and stalls
//! holds a task for [`HANDSHAKE_TIMEOUT`], not the accept loop.
//!
//! [`ConnectInfo<TlsPeer>`]: axum::extract::ConnectInfo

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::error::AppError;
use crate::middleware::sanitize_for_log;

/// Upper bound on a TLS handshake; slower clients are dropped.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the server's certificate chain, private key and the CA that
/// client certificates must chain to live on disk (`TLS_CERT`, `TLS_KEY`,
/// `TLS_CLIENT_CA`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    /// PEM: server leaf certificate first, then intermediates.
    pub cert: PathBuf,
    /// PEM: the server's private key (PKCS#8, SEC1 or PKCS#1).
    pub key: PathBuf,
    /// PEM: one or more CA certificates clients must chain to.
    pub client_ca: PathBuf,
}

/// The other end of an accepted mTLS connection. Available to handlers
/// and middleware as `ConnectInfo<TlsPeer>`; `subject` is the verified
/// peer certificate's subject DN, formatted by `x509-parser`
/// (`CN=dev-1, OU=devices, O=Supervictor`).
#[derive(Debug, Clone)]
pub struct TlsPeer {
    /// Remote socket address.
    pub remote: SocketAddr,
    /// Subject DN of the verified client certificate. `None` only if the
    /// certificate could not be parsed after verification, which webpki
    /// makes practically impossible; callers treat it as "no identity".
    pub subject: Option<String>,
}

/// Subject DN of a DER certificate, `None` if it does not parse.
pub fn subject_dn(cert: &CertificateDer<'_>) -> Option<String> {
    x509_parser::parse_x509_certificate(cert)
        .ok()
        .map(|(_, parsed)| parsed.subject().to_string())
}

fn tls_err(what: &str, path: &Path, err: impl std::fmt::Display) -> AppError {
    AppError::Config(format!("{what} {}: {err}", path.display()))
}

/// Build the rustls server configuration: ring provider, TLS 1.2/1.3,
/// client certificates required and verified against `client_ca`.
pub fn server_config(paths: &TlsPaths) -> Result<Arc<ServerConfig>, AppError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&paths.cert)
        .map_err(|e| tls_err("cannot read TLS_CERT", &paths.cert, e))?
        .collect::<Result<_, _>>()
        .map_err(|e| tls_err("malformed TLS_CERT", &paths.cert, e))?;
    if certs.is_empty() {
        return Err(tls_err("no certificates in TLS_CERT", &paths.cert, "empty"));
    }
    let key = PrivateKeyDer::from_pem_file(&paths.key)
        .map_err(|e| tls_err("cannot read TLS_KEY", &paths.key, e))?;

    let mut roots = RootCertStore::empty();
    let mut ca_count = 0usize;
    for ca in CertificateDer::pem_file_iter(&paths.client_ca)
        .map_err(|e| tls_err("cannot read TLS_CLIENT_CA", &paths.client_ca, e))?
    {
        let ca = ca.map_err(|e| tls_err("malformed TLS_CLIENT_CA", &paths.client_ca, e))?;
        roots
            .add(ca)
            .map_err(|e| tls_err("rejected CA in TLS_CLIENT_CA", &paths.client_ca, e))?;
        ca_count += 1;
    }
    if ca_count == 0 {
        return Err(tls_err(
            "no certificates in TLS_CLIENT_CA",
            &paths.client_ca,
            "empty",
        ));
    }

    // Mandatory client auth (no `allow_unauthenticated`): a connection
    // without a certificate chaining to the CA never completes.
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|e| AppError::Config(format!("client verifier: {e}")))?;

    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::Config(format!("TLS protocol versions: {e}")))?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|e| AppError::Config(format!("TLS_CERT/TLS_KEY mismatch: {e}")))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// An [`axum::serve::Listener`] that yields fully handshaken mTLS
/// connections together with the peer's verified identity.
pub struct TlsListener {
    local_addr: SocketAddr,
    ready: mpsc::Receiver<(TlsStream<TcpStream>, TlsPeer)>,
}

impl TlsListener {
    /// Bind `addr` and start accepting. Handshakes are spawned per
    /// connection; failures are logged and dropped, never surfaced to
    /// the server loop.
    pub async fn bind(addr: &str, paths: &TlsPaths) -> Result<Self, AppError> {
        let config = server_config(paths)?;
        let tcp = TcpListener::bind(addr)
            .await
            .map_err(|e| AppError::Config(format!("bind {addr}: {e}")))?;
        Ok(Self::from_parts(tcp, config))
    }

    /// Wrap an already-bound TCP listener (tests bind port 0 and read
    /// [`Self::local_addr`]).
    pub fn from_parts(tcp: TcpListener, config: Arc<ServerConfig>) -> Self {
        let local_addr = tcp
            .local_addr()
            .expect("bound TcpListener has a local address");
        let acceptor = TlsAcceptor::from(config);
        let (tx, ready) = mpsc::channel(64);
        tokio::spawn(accept_loop(tcp, acceptor, tx));
        Self { local_addr, ready }
    }
}

async fn accept_loop(
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    tx: mpsc::Sender<(TlsStream<TcpStream>, TlsPeer)>,
) {
    loop {
        let (stream, remote) = match tcp.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                // Same policy as axum's TcpListener impl: transient accept
                // errors (EMFILE, ECONNABORTED) back off instead of spinning.
                tracing::warn!(error = %e, "accept failed");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(tls)) => {
                    let subject = tls
                        .get_ref()
                        .1
                        .peer_certificates()
                        .and_then(|chain| chain.first())
                        .and_then(subject_dn);
                    tracing::debug!(
                        %remote,
                        subject = %sanitize_for_log(subject.as_deref().unwrap_or("<none>")),
                        "mTLS connection established"
                    );
                    // The receiver only goes away at shutdown; dropping the
                    // connection then is the right outcome.
                    let _ = tx.send((tls, TlsPeer { remote, subject })).await;
                }
                Ok(Err(e)) => {
                    tracing::warn!(%remote, error = %e, "TLS handshake failed");
                }
                Err(_) => {
                    tracing::warn!(%remote, "TLS handshake timed out");
                }
            }
        });
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = TlsPeer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(conn) => conn,
            // Only reachable if the accept loop is gone, which only
            // happens when the runtime is shutting down; park rather than
            // hand the server a bogus connection.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(TlsPeer {
            remote: self.local_addr,
            subject: None,
        })
    }
}

/// Lets `Router::into_make_service_with_connect_info::<TlsPeer>()` attach
/// the verified identity to every request on the connection.
impl Connected<IncomingStream<'_, TlsListener>> for TlsPeer {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

impl TlsListener {
    /// The bound socket address.
    pub fn socket_addr(&self) -> SocketAddr {
        self.local_addr
    }
}
