//! In-binary mTLS, end to end over a REAL TLS socket: certificates minted
//! by the repo's own `scripts/gen_certs.sh` (openssl), a `TlsListener`
//! bound on 127.0.0.1:0, raw HTTP/1.1 over `tokio-rustls` client
//! connections. No mocked handshakes.
//!
//! What this pins: identity comes from the verified peer certificate and
//! from nowhere else — forged subject headers are ignored, a certificate
//! from another CA never completes the handshake, and no certificate at
//! all never completes the handshake.
#![cfg(feature = "tls")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use rustls::ClientConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use supervictor_endpoint::error::AppError;
use supervictor_endpoint::middleware::dn_matches;
use supervictor_endpoint::models::DeviceRecord;
use supervictor_endpoint::routes::router;
use supervictor_endpoint::store::DeviceStore;
use supervictor_endpoint::tls::{server_config, subject_dn, TlsListener, TlsPaths, TlsPeer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;

// ── PKI fixture ──────────────────────────────────────────────────────

struct Pki {
    dir: PathBuf,
}

impl Pki {
    /// CA + two devices + one admin + a server cert for `localhost`,
    /// exactly as `Quickstart certs` would issue them.
    fn mint(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sv-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pki = Self { dir };
        pki.gen(&["ca"]);
        pki.gen(&["device", "dev-1"]);
        pki.gen(&["device", "dev-2"]);
        pki.gen(&["admin", "ops"]);
        pki.gen(&["server", "local", "localhost"]);
        pki
    }

    fn gen(&self, args: &[&str]) {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/gen_certs.sh");
        let out = Command::new("bash")
            .arg(&script)
            .args(args)
            .env("CERTS_DIR", &self.dir)
            .current_dir(&self.dir)
            .output()
            .expect("spawn bash");
        assert!(
            out.status.success(),
            "gen_certs.sh {args:?} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn ca(&self) -> PathBuf {
        self.dir.join("ca/ca.pem")
    }
    fn device(&self, name: &str) -> (PathBuf, PathBuf) {
        let d = self.dir.join("devices").join(name);
        (d.join("client.pem"), d.join("client.key"))
    }
    fn admin(&self, name: &str) -> (PathBuf, PathBuf) {
        let d = self.dir.join("admins").join(name);
        (d.join("admin.pem"), d.join("admin.key"))
    }
    fn server_paths(&self) -> TlsPaths {
        let d = self.dir.join("servers/local");
        TlsPaths {
            cert: d.join("server.pem"),
            key: d.join("server.key"),
            client_ca: self.ca(),
        }
    }

    /// The subject as `Quickstart onboard` registers it (openssl, RFC 2253).
    fn rfc2253_subject(&self, pem: &Path) -> String {
        let out = Command::new("openssl")
            .args(["x509", "-in"])
            .arg(pem)
            .args(["-noout", "-subject", "-nameopt", "RFC2253"])
            .output()
            .expect("spawn openssl");
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_start_matches("subject=")
            .to_string()
    }
}

// ── Server + client helpers ──────────────────────────────────────────

async fn start_server(pki: &Pki, store: Arc<dyn DeviceStore>) -> std::net::SocketAddr {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener = TlsListener::from_parts(tcp, server_config(&pki.server_paths()).unwrap());
    let addr = listener.socket_addr();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(store).into_make_service_with_connect_info::<TlsPeer>(),
        )
        .await
        .unwrap();
    });
    addr
}

fn client_config(ca: &Path, identity: Option<(&Path, &Path)>) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(ca).unwrap() {
        roots.add(cert.unwrap()).unwrap();
    }
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots);
    let mut config = match identity {
        Some((cert, key)) => {
            let certs = CertificateDer::pem_file_iter(cert)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let key = PrivateKeyDer::from_pem_file(key).unwrap();
            builder.with_client_auth_cert(certs, key).unwrap()
        }
        None => builder.with_no_client_auth(),
    };
    // Offer what curl and browsers offer. The server must settle on
    // http/1.1 — it cannot speak h2 — and the raw HTTP/1.1 below must
    // then work. (Advertising h2 server-side once broke every curl.)
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

/// One HTTP/1.1 request over a fresh mTLS connection. `Err` when the
/// handshake (or the connection) fails before a response arrives.
async fn request(
    addr: std::net::SocketAddr,
    config: Arc<ClientConfig>,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &str,
) -> Result<(u16, String), String> {
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let connector = TlsConnector::from(config);
    let mut tls = connector
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    assert_eq!(
        tls.get_ref().1.alpn_protocol(),
        Some(&b"http/1.1"[..]),
        "server must negotiate http/1.1 even when h2 is offered"
    );
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in extra_headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    tls.write_all(req.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = Vec::new();
    tls.read_to_end(&mut raw)
        .await
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("no status line in {text:?}"))?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

fn record(id: &str, subject_dn: Option<&str>) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "owner-1".into(),
        subject_dn: subject_dn.map(str::to_string),
        status: "active".into(),
        created_at: "2026-07-01T00:00:00+00:00".into(),
        power_threshold_ma: None,
        min_on_s: None,
        style: Default::default(),
        desired_fw: None,
    }
}

fn uplink_body(id: &str) -> String {
    format!(r#"{{"id":"{id}","current":1}}"#)
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn bound_device_posts_with_its_own_certificate() {
    let pki = Pki::mint("own-cert");
    let (cert, key) = pki.device("dev-1");
    let store = common::test_store();
    store
        .put_device(record("dev-1", Some(&pki.rfc2253_subject(&cert))))
        .unwrap();
    let addr = start_server(&pki, store.clone()).await;

    let (status, body) = request(
        addr,
        client_config(&pki.ca(), Some((&cert, &key))),
        "POST",
        "/",
        &[],
        &uplink_body("dev-1"),
    )
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        dn_matches(
            json["client_subject"].as_str().unwrap(),
            &pki.rfc2253_subject(&cert)
        ),
        "client_subject must be the verified peer subject: {body}"
    );
    assert_eq!(store.get_uplinks("dev-1", 10).unwrap().len(), 1);
}

#[tokio::test]
async fn another_devices_certificate_is_rejected() {
    let pki = Pki::mint("other-cert");
    let (cert1, _) = pki.device("dev-1");
    let (cert2, key2) = pki.device("dev-2");
    let store = common::test_store();
    store
        .put_device(record("dev-1", Some(&pki.rfc2253_subject(&cert1))))
        .unwrap();
    let addr = start_server(&pki, store.clone()).await;

    let (status, body) = request(
        addr,
        client_config(&pki.ca(), Some((&cert2, &key2))),
        "POST",
        "/",
        &[],
        &uplink_body("dev-1"),
    )
    .await
    .unwrap();
    assert_eq!(status, 403, "{body}");
    assert!(
        body.contains("Client certificate does not match device"),
        "{body}"
    );
    assert!(store.get_uplinks("dev-1", 10).unwrap().is_empty());
}

/// The whole point of terminating TLS in-process: headers that a proxy
/// would have set are now attacker-controlled, so they must be ignored.
#[tokio::test]
async fn forged_subject_headers_are_ignored_in_tls_mode() {
    let pki = Pki::mint("forged");
    let (cert1, _) = pki.device("dev-1");
    let (cert2, key2) = pki.device("dev-2");
    let dn1 = pki.rfc2253_subject(&cert1);
    let store = common::test_store();
    store.put_device(record("dev-1", Some(&dn1))).unwrap();
    let addr = start_server(&pki, store.clone()).await;
    let cfg = client_config(&pki.ca(), Some((&cert2, &key2)));

    let lwa = format!(r#"{{"identity":{{"clientCert":{{"subjectDN":"{dn1}"}}}}}}"#);
    for headers in [
        vec![("x-ssl-client-subject-dn", dn1.as_str())],
        vec![("x-amzn-request-context", lwa.as_str())],
        vec![
            ("x-ssl-client-subject-dn", dn1.as_str()),
            ("x-amzn-request-context", lwa.as_str()),
        ],
    ] {
        let (status, body) = request(
            addr,
            cfg.clone(),
            "POST",
            "/",
            &headers,
            &uplink_body("dev-1"),
        )
        .await
        .unwrap();
        assert_eq!(
            status, 403,
            "headers {headers:?} must not grant dev-1: {body}"
        );
    }
    assert!(store.get_uplinks("dev-1", 10).unwrap().is_empty());

    // And the forged header doesn't even leak into the echoed subject.
    let (status, body) = request(
        addr,
        cfg,
        "GET",
        "/",
        &[("x-ssl-client-subject-dn", "CN=ops,OU=admin,O=Supervictor")],
        "",
    )
    .await
    .unwrap();
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        dn_matches(
            json["client_subject"].as_str().unwrap(),
            &pki.rfc2253_subject(&cert2)
        ),
        "{body}"
    );
}

#[tokio::test]
async fn no_client_certificate_never_completes() {
    let pki = Pki::mint("no-cert");
    let addr = start_server(&pki, common::test_store()).await;
    let err = request(
        addr,
        client_config(&pki.ca(), None),
        "GET",
        "/health",
        &[],
        "",
    )
    .await
    .expect_err("a connection without a client certificate must not get a response");
    assert!(
        err.starts_with("handshake") || err.starts_with("read") || err.starts_with("write"),
        "{err}"
    );
}

#[tokio::test]
async fn certificate_from_another_ca_never_completes() {
    let pki = Pki::mint("good-ca");
    let rogue = Pki::mint("rogue-ca");
    let (cert, key) = rogue.device("dev-1");
    let addr = start_server(&pki, common::test_store()).await;
    // The rogue client even trusts the real server; the server must not
    // trust it back.
    let err = request(
        addr,
        client_config(&pki.ca(), Some((&cert, &key))),
        "GET",
        "/health",
        &[],
        "",
    )
    .await
    .expect_err("a certificate from an unknown CA must not get a response");
    assert!(
        err.starts_with("handshake") || err.starts_with("read") || err.starts_with("write"),
        "{err}"
    );
}

#[tokio::test]
async fn admin_routes_need_an_admin_certificate() {
    let pki = Pki::mint("admin");
    let (admin_cert, admin_key) = pki.admin("ops");
    let (dev_cert, dev_key) = pki.device("dev-1");
    let addr = start_server(&pki, common::test_store()).await;

    let (status, _) = request(
        addr,
        client_config(&pki.ca(), Some((&admin_cert, &admin_key))),
        "GET",
        "/fleet",
        &[],
        "",
    )
    .await
    .unwrap();
    assert_eq!(status, 200, "OU=admin certificate reaches /fleet");

    let dev = client_config(&pki.ca(), Some((&dev_cert, &dev_key)));
    let (status, _) = request(addr, dev.clone(), "GET", "/fleet", &[], "")
        .await
        .unwrap();
    assert_eq!(status, 403, "device certificate is not admin");
    let (status, _) = request(
        addr,
        dev,
        "GET",
        "/fleet",
        &[("x-ssl-client-subject-dn", "CN=ops,OU=admin,O=Supervictor")],
        "",
    )
    .await
    .unwrap();
    assert_eq!(status, 403, "a forged admin header must not escalate");
}

#[tokio::test]
async fn subject_dn_parses_what_openssl_issued() {
    let pki = Pki::mint("subject");
    let (cert, _) = pki.device("dev-1");
    let der = CertificateDer::from_pem_file(&cert).unwrap();
    let parsed = subject_dn(&der).expect("parses");
    assert!(
        dn_matches(&parsed, &pki.rfc2253_subject(&cert)),
        "x509-parser {parsed:?} vs openssl {:?}",
        pki.rfc2253_subject(&cert)
    );
    assert!(subject_dn(&CertificateDer::from(vec![0x30, 0x00])).is_none());
}

#[test]
fn server_config_rejects_bad_material() {
    let pki = Pki::mint("badcfg");
    let good = pki.server_paths();

    let missing = TlsPaths {
        cert: pki.dir.join("nope.pem"),
        ..good.clone()
    };
    let err = server_config(&missing).unwrap_err();
    assert!(
        matches!(err, AppError::Config(ref m) if m.contains("TLS_CERT")),
        "{err}"
    );

    let (_, other_key) = pki.device("dev-2");
    let mismatched = TlsPaths {
        key: other_key,
        ..good.clone()
    };
    let err = server_config(&mismatched).unwrap_err();
    assert!(
        matches!(err, AppError::Config(ref m) if m.contains("mismatch")),
        "{err}"
    );

    let empty = pki.dir.join("empty.pem");
    std::fs::write(&empty, "").unwrap();
    let no_ca = TlsPaths {
        client_ca: empty.clone(),
        ..good.clone()
    };
    let err = server_config(&no_ca).unwrap_err();
    assert!(
        matches!(err, AppError::Config(ref m) if m.contains("TLS_CLIENT_CA")),
        "{err}"
    );

    let garbage = pki.dir.join("garbage.pem");
    std::fs::write(
        &garbage,
        "-----BEGIN CERTIFICATE-----\nnot base64!\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    let bad_cert = TlsPaths {
        cert: garbage,
        ..good.clone()
    };
    assert!(server_config(&bad_cert).is_err());

    server_config(&good).expect("the real material loads");
}
