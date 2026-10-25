//! One mTLS request at a time against an endpoint.
//!
//! Each call opens a TCP connection, completes the TLS handshake with the
//! admin identity, writes one GET, reads to EOF and parses. The whole thing
//! runs under one deadline so a wedged server cannot hang the MCP client.
//! Errors are phrased for the operator reading them through a model: which
//! stage failed and which `SUPERVICTOR_*` variable or command fixes it.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::ClientConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::config::{Config, Target};
use crate::http::{self, Response};

/// What one GET produced, plus the transport facts a status tool reports.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Parsed response.
    pub response: Response,
    /// Connect + handshake + request + read, in milliseconds.
    pub latency_ms: u64,
    /// Negotiated TLS protocol, e.g. `TLSv1_3`.
    pub tls_version: String,
}

impl Fetched {
    /// The body as JSON, or an error naming the status and a body excerpt.
    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.response.body).map_err(|e| {
            format!(
                "HTTP {} with a non-JSON body ({e}): {}",
                self.response.status,
                excerpt(&self.response.body)
            )
        })
    }
}

/// Up to 200 bytes of a body for an error message, control characters
/// stripped so a hostile server cannot inject lines into the model's view.
pub fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let cleaned: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    if text.chars().count() > 200 {
        format!("{cleaned}…")
    } else {
        cleaned
    }
}

/// A configured mTLS client. Cheap to clone (the TLS config is shared).
#[derive(Clone)]
pub struct Client {
    connector: TlsConnector,
    timeout: Duration,
    ca_path: String,
    cert_path: String,
}

impl Client {
    /// Load the CA bundle and the admin identity named in `config`.
    pub fn new(config: &Config) -> Result<Self, String> {
        Self::from_paths(&config.ca, &config.cert, &config.key, config.timeout)
    }

    /// Load from explicit paths.
    pub fn from_paths(
        ca: &Path,
        cert: &Path,
        key: &Path,
        timeout: Duration,
    ) -> Result<Self, String> {
        let mut roots = rustls::RootCertStore::empty();
        let mut ca_count = 0;
        for item in CertificateDer::pem_file_iter(ca).map_err(|e| {
            format!(
                "cannot read the CA bundle at {} ({e}); set SUPERVICTOR_CA to the PEM the \
                 endpoint's server certificate chains to (the fleet CA for a self-hosted \
                 endpoint, a public root for a hosted one)",
                ca.display()
            )
        })? {
            let der = item.map_err(|e| format!("{}: bad PEM: {e}", ca.display()))?;
            roots
                .add(der)
                .map_err(|e| format!("{}: not a CA certificate: {e}", ca.display()))?;
            ca_count += 1;
        }
        if ca_count == 0 {
            return Err(format!(
                "{} holds no certificates; SUPERVICTOR_CA must be a PEM CA bundle",
                ca.display()
            ));
        }

        let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
            .map_err(|e| {
                format!(
                    "cannot read the admin certificate at {} ({e}); set SUPERVICTOR_ADMIN_CERT, \
                     or issue one with: ./Quickstart certs admin <name>",
                    cert.display()
                )
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("{}: bad PEM: {e}", cert.display()))?;
        if certs.is_empty() {
            return Err(format!("{} holds no certificate", cert.display()));
        }
        let key = PrivateKeyDer::from_pem_file(key).map_err(|e| {
            format!(
                "cannot read the admin private key at {} ({e}); set SUPERVICTOR_ADMIN_KEY",
                key.display()
            )
        })?;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS configuration: {e}"))?
            .with_root_certificates(roots)
            .with_client_auth_cert(certs, key)
            .map_err(|e| {
                format!(
                    "admin certificate and key do not form a usable identity ({e}); \
                     check SUPERVICTOR_ADMIN_CERT / SUPERVICTOR_ADMIN_KEY point at the same \
                     certs/admins/<name>/ pair"
                )
            })?;
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            connector: TlsConnector::from(Arc::new(tls)),
            timeout,
            ca_path: ca.display().to_string(),
            cert_path: cert.display().to_string(),
        })
    }

    /// `GET route` on `target` (route is appended to the target's prefix).
    pub async fn get(&self, target: &Target, route: &str) -> Result<Fetched, String> {
        let started = Instant::now();
        let result = tokio::time::timeout(self.timeout, self.get_inner(target, route)).await;
        match result {
            Ok(Ok((response, tls_version))) => Ok(Fetched {
                response,
                latency_ms: started.elapsed().as_millis() as u64,
                tls_version,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!(
                "no response from {} within {}s (SUPERVICTOR_TIMEOUT_SECS); the host is \
                 reachable but the endpoint is not answering",
                target.display(),
                self.timeout.as_secs()
            )),
        }
    }

    async fn get_inner(&self, target: &Target, route: &str) -> Result<(Response, String), String> {
        let endpoint = target.display();
        let tcp = TcpStream::connect(target.dial()).await.map_err(|e| {
            let hint = match e.kind() {
                std::io::ErrorKind::ConnectionRefused => {
                    "nothing is listening there — is the endpoint running? \
                     (`./Quickstart status` for a local one, `systemctl status \
                     supervictor-endpoint` on a box)"
                }
                _ if e.to_string().contains("resolve") || e.to_string().contains("nodename") => {
                    "the hostname does not resolve from this machine"
                }
                _ => "check the host, the port, and the network path from this machine",
            };
            format!("cannot connect to {endpoint}: {e}; {hint}")
        })?;
        let server_name = ServerName::try_from(target.host.clone())
            .map_err(|e| format!("{}: not a valid TLS server name: {e}", target.host))?;
        let mut tls = self
            .connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| {
                handshake_error(&endpoint, &e.to_string(), &self.ca_path, &self.cert_path)
            })?;
        let tls_version = tls
            .get_ref()
            .1
            .protocol_version()
            .map(|v| format!("{v:?}"))
            .unwrap_or_else(|| "unknown".into());
        let request = http::get_request(&target.host_header(), &target.path(route));
        tls.write_all(request.as_bytes())
            .await
            .map_err(|e| format!("{endpoint}: writing the request failed: {e}"))?;
        let mut raw = Vec::new();
        // A server that rejects the client certificate after the handshake
        // (TLS 1.3 delivers the alert on first read) shows up here, not above.
        if let Err(e) = tls.read_to_end(&mut raw).await {
            if raw.is_empty() {
                return Err(handshake_error(
                    &endpoint,
                    &e.to_string(),
                    &self.ca_path,
                    &self.cert_path,
                ));
            }
            // Many servers close without a TLS close_notify; the bytes we
            // have are still a complete response if the parser agrees.
        }
        let response = http::parse_response(&raw).map_err(|e| format!("{endpoint}: {e}"))?;
        Ok((response, tls_version))
    }
}

/// Map a rustls error string to advice. The strings are rustls's own
/// `Display` output; matching on them keeps this independent of the
/// error enum's shape across versions.
fn handshake_error(endpoint: &str, err: &str, ca_path: &str, cert_path: &str) -> String {
    let lower = err.to_ascii_lowercase();
    let hint = if lower.contains("unknownissuer")
        || lower.contains("unknown issuer")
        || lower.contains("badsignature")
    {
        // BadSignature is what a CA with the SAME subject but a different
        // key produces (two "Supervictor Root CA"s is a known state of the
        // world), so it gets the same advice as an unknown issuer.
        format!(
            "the server certificate does not chain to {ca_path} (a CA with the same name but a \
             different key counts as a different CA); set SUPERVICTOR_CA to the CA that signed \
             the endpoint's server certificate"
        )
    } else if lower.contains("notvalidforname") || lower.contains("not valid for name") {
        "the server certificate is for a different hostname than the one in the URL; use the \
         name the certificate was issued for (its SAN), or reissue it: ./Quickstart certs \
         server <name> <host>"
            .to_string()
    } else if lower.contains("expired") || lower.contains("notvalidyet") {
        "a certificate is outside its validity period; check the clocks and the expiry dates \
         with `openssl x509 -noout -dates -in <pem>`"
            .to_string()
    } else if lower.contains("certificaterequired")
        || lower.contains("certificate required")
        || lower.contains("handshakefailure")
        || lower.contains("unknownca")
        || lower.contains("bad certificate")
        || lower.contains("badcertificate")
        || lower.contains("accessdenied")
        // The alert a rustls server sends when the client certificate's
        // signature does not verify against its CA (same-name foreign CA).
        || lower.contains("decrypterror")
    {
        format!(
            "the endpoint rejected the client certificate at {cert_path}; it must be issued by \
             the CA the endpoint trusts (TLS_CLIENT_CA) — reissue with ./Quickstart certs admin \
             <name> from that CA"
        )
    } else {
        "check that the endpoint terminates TLS itself (TLS_CERT/TLS_KEY/TLS_CLIENT_CA) or sits \
         behind a proxy that does"
            .to_string()
    };
    format!("TLS handshake with {endpoint} failed: {err}; {hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_strips_controls_and_caps_length() {
        assert_eq!(excerpt(b"ok\n\x1b[31mred"), "ok[31mred");
        let long = vec![b'a'; 300];
        let e = excerpt(&long);
        assert_eq!(e.chars().count(), 201);
        assert!(e.ends_with('…'));
    }

    #[test]
    fn handshake_hints_name_the_fix() {
        let e = handshake_error(
            "https://h",
            "invalid peer certificate: UnknownIssuer",
            "ca.pem",
            "a.pem",
        );
        assert!(e.contains("SUPERVICTOR_CA"), "{e}");
        let e = handshake_error(
            "https://h",
            "invalid peer certificate: NotValidForName",
            "ca.pem",
            "a.pem",
        );
        assert!(e.contains("SAN"), "{e}");
        let e = handshake_error(
            "https://h",
            "received fatal alert: CertificateRequired",
            "ca.pem",
            "a.pem",
        );
        assert!(e.contains("a.pem"), "{e}");
        let e = handshake_error("https://h", "something else", "ca.pem", "a.pem");
        assert!(e.contains("TLS_CERT"), "{e}");
    }

    #[test]
    fn missing_files_are_named() {
        let dir = std::env::temp_dir();
        let err = Client::from_paths(
            &dir.join("nope-ca.pem"),
            &dir.join("nope.pem"),
            &dir.join("nope.key"),
            Duration::from_secs(1),
        )
        .err()
        .expect("a missing CA bundle must fail");
        assert!(
            err.contains("nope-ca.pem") && err.contains("SUPERVICTOR_CA"),
            "{err}"
        );
    }
}
