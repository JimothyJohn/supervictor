use std::path::{Path, PathBuf};

use ureq::tls::{Certificate, ClientCert, PrivateKey, RootCerts, TlsConfig};

use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::runner;

/// Arguments for the `qs ping` command.
pub struct PingArgs {
    /// Optional path to the device certs directory (default: `certs/devices/test-device`).
    pub certs: Option<PathBuf>,
    /// Optional CA cert path (default: WebPki roots).
    pub ca: Option<PathBuf>,
    /// Target hostname.
    pub host: String,
    /// Target HTTPS port.
    pub port: u16,
    /// Print commands without executing.
    pub dry_run: bool,
}

/// Send an mTLS GET request to the target endpoint and print the response.
pub fn run_ping(args: &PingArgs, config: &ProjectConfig) -> Result<i32, CliError> {
    let cert_dir = args
        .certs
        .clone()
        .unwrap_or_else(|| config.repo_root.join("certs/devices/test-device"));
    let ca = args.ca.clone();

    let client_cert = cert_dir.join("client.pem");
    let client_key = cert_dir.join("client.key");

    for (path, label) in [(&client_cert, "client cert"), (&client_key, "client key")] {
        if !path.exists() {
            runner::error(&format!("{} not found at {}", label, path.display()));
            return Ok(1);
        }
    }

    if let Some(ref ca_path) = ca {
        if !ca_path.exists() {
            runner::error(&format!("CA cert not found at {}", ca_path.display()));
            return Ok(1);
        }
    }

    let url = format!("https://{}:{}/", args.host, args.port);
    runner::step(&format!("Pinging {}", url));

    if args.dry_run {
        println!("  [dry-run] GET {}", url);
        return Ok(0);
    }

    let agent = build_mtls_agent(ca.as_deref(), &client_cert, &client_key)?;

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

/// Build a ureq Agent configured for mTLS.
/// Uses WebPki roots by default; if `ca_path` is provided, uses that CA exclusively.
pub fn build_mtls_agent(
    ca_path: Option<&Path>,
    cert_path: &Path,
    key_path: &Path,
) -> Result<ureq::Agent, CliError> {
    let cert_pem = std::fs::read(cert_path)?;
    let client_cert = Certificate::from_pem(&cert_pem)
        .map_err(|e| CliError::Config(format!("failed to parse client cert: {}", e)))?;

    let key_pem = std::fs::read(key_path)?;
    let client_key = PrivateKey::from_pem(&key_pem)
        .map_err(|e| CliError::Config(format!("failed to parse client key: {}", e)))?;

    let root_certs = match ca_path {
        Some(path) => {
            let ca_pem = std::fs::read(path)?;
            let ca_cert = Certificate::from_pem(&ca_pem)
                .map_err(|e| CliError::Config(format!("failed to parse CA cert: {}", e)))?;
            RootCerts::Specific(std::sync::Arc::new(vec![ca_cert]))
        }
        None => RootCerts::WebPki,
    };

    let tls_config = TlsConfig::builder()
        .root_certs(root_certs)
        .client_cert(Some(ClientCert::new_with_certs(&[client_cert], client_key)))
        .build();

    let config = ureq::Agent::config_builder().tls_config(tls_config).build();

    Ok(ureq::Agent::new_with_config(config))
}

/// Build a plain ureq Agent with no mTLS.
pub fn build_plain_agent() -> ureq::Agent {
    ureq::Agent::new_with_defaults()
}
