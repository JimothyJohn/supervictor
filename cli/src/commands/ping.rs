use std::path::{Path, PathBuf};
use std::sync::Arc;

use ureq::tls::{Certificate, ClientCert, PrivateKey, RootCerts, TlsConfig};

use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::runner;

/// Arguments for the ping command.
pub struct PingArgs {
    pub certs: Option<PathBuf>,
    pub ca: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub dry_run: bool,
}

pub fn run_ping(args: &PingArgs, config: &ProjectConfig) -> Result<i32, CliError> {
    let cert_dir = args
        .certs
        .clone()
        .unwrap_or_else(|| config.repo_root.join("certs/devices/test-device"));
    let ca = args
        .ca
        .clone()
        .unwrap_or_else(|| config.repo_root.join("certs/ca/ca.pem"));

    let client_cert = cert_dir.join("client.pem");
    let client_key = cert_dir.join("client.key");

    for (path, label) in [
        (&client_cert, "client cert"),
        (&client_key, "client key"),
        (&ca, "CA cert"),
    ] {
        if !path.exists() {
            runner::error(&format!("{} not found at {}", label, path.display()));
            return Ok(1);
        }
    }

    let url = format!("https://{}:{}/", args.host, args.port);
    runner::step(&format!("Pinging {}", url));

    if args.dry_run {
        println!("  [dry-run] GET {}", url);
        return Ok(0);
    }

    let agent = build_mtls_agent(&ca, &client_cert, &client_key)?;

    match agent.get(&url).call() {
        Ok(response) => {
            let status = response.status();
            let body = response.into_body().read_to_string().unwrap_or_default();
            runner::success(&format!("Status: {}", status));
            println!("{}", body);
            Ok(0)
        }
        Err(ureq::Error::StatusCode(status)) => {
            runner::success(&format!("Status: {}", status));
            Ok(0)
        }
        Err(e) => {
            runner::error(&format!("Error: {}", e));
            Ok(1)
        }
    }
}

/// Build a ureq Agent configured for mTLS with custom CA and client cert.
pub fn build_mtls_agent(
    ca_path: &Path,
    cert_path: &Path,
    key_path: &Path,
) -> Result<ureq::Agent, CliError> {
    let ca_pem = std::fs::read(ca_path)?;
    let ca_cert = Certificate::from_pem(&ca_pem)
        .map_err(|e| CliError::Config(format!("failed to parse CA cert: {}", e)))?;

    let cert_pem = std::fs::read(cert_path)?;
    let client_cert = Certificate::from_pem(&cert_pem)
        .map_err(|e| CliError::Config(format!("failed to parse client cert: {}", e)))?;

    let key_pem = std::fs::read(key_path)?;
    let client_key = PrivateKey::from_pem(&key_pem)
        .map_err(|e| CliError::Config(format!("failed to parse client key: {}", e)))?;

    let tls_config = TlsConfig::builder()
        .root_certs(RootCerts::Specific(Arc::new(vec![ca_cert])))
        .client_cert(Some(ClientCert::new_with_certs(&[client_cert], client_key)))
        .build();

    let config = ureq::Agent::config_builder()
        .tls_config(tls_config)
        .build();

    Ok(ureq::Agent::new_with_config(config))
}

/// Build a plain ureq Agent with no mTLS.
pub fn build_plain_agent() -> ureq::Agent {
    ureq::Agent::new_with_defaults()
}
