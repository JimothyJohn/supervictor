use std::env;
use std::path::PathBuf;

use crate::error::AppError;

/// Paths for in-binary mTLS termination (`TLS_CERT`, `TLS_KEY`,
/// `TLS_CLIENT_CA`). All three or none: a partial set is a configuration
/// error, never a silent fallback to plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsConfig {
    /// Server certificate chain, PEM.
    pub cert: PathBuf,
    /// Server private key, PEM.
    pub key: PathBuf,
    /// CA bundle client certificates must chain to, PEM.
    pub client_ca: PathBuf,
}

/// Runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Deployment environment (e.g. `dev`, `staging`, `prod`).
    pub environment: String,
    /// Application name used in logging and metadata.
    pub app_name: String,
    /// Tracing log level filter (e.g. `info`, `debug`).
    pub log_level: String,
    /// TCP port to bind the HTTP listener on.
    pub port: u16,
    /// Storage backend selection (`sqlite` or `dynamo`).
    pub store_backend: String,
    /// DynamoDB table name for device records.
    pub devices_table: String,
    /// DynamoDB table name for uplink messages.
    pub messages_table: String,
    /// Filesystem path (or `:memory:`) for the SQLite database.
    pub sqlite_db_path: String,
    /// Staleness watchdog tick interval in seconds; 0 disables it.
    pub watchdog_interval_secs: u64,
    /// Days before an uplink row expires (DynamoDB TTL); 0 keeps rows forever.
    pub uplink_ttl_days: u32,
    /// Reject uplinks from devices that are not registered, `active`, and
    /// bound to a certificate (`subject_dn` on record). Certificate binding
    /// for devices that *are* bound is enforced regardless of this flag.
    pub require_registration: bool,
    /// Terminate mTLS in-process when set (single-binary deployment);
    /// plain HTTP behind API Gateway / a proxy when `None`.
    pub tls: Option<TlsConfig>,
}

/// `TLS_CERT` + `TLS_KEY` + `TLS_CLIENT_CA` → `Some(TlsConfig)`; none set →
/// `None`; anything in between → error naming the missing variables.
fn tls_from_env() -> Result<Option<TlsConfig>, AppError> {
    let read = |name: &str| env::var(name).ok().filter(|v| !v.trim().is_empty());
    let (cert, key, ca) = (read("TLS_CERT"), read("TLS_KEY"), read("TLS_CLIENT_CA"));
    match (cert, key, ca) {
        (None, None, None) => Ok(None),
        (Some(cert), Some(key), Some(ca)) => Ok(Some(TlsConfig {
            cert: PathBuf::from(cert),
            key: PathBuf::from(key),
            client_ca: PathBuf::from(ca),
        })),
        (cert, key, ca) => {
            let missing: Vec<&str> = [
                ("TLS_CERT", cert.is_none()),
                ("TLS_KEY", key.is_none()),
                ("TLS_CLIENT_CA", ca.is_none()),
            ]
            .into_iter()
            .filter_map(|(name, absent)| absent.then_some(name))
            .collect();
            Err(AppError::Config(format!(
                "incomplete TLS configuration: missing {}",
                missing.join(", ")
            )))
        }
    }
}

/// Parse a boolean environment value: `true`/`false`, `1`/`0`, `yes`/`no`,
/// `on`/`off` (case-insensitive); empty means `false`. Anything else is a
/// configuration error rather than a silent `false` — a typo in a security
/// knob must fail the boot, not disable the check.
fn parse_bool(name: &str, raw: &str) -> Result<bool, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "false" | "0" | "no" | "off" => Ok(false),
        "true" | "1" | "yes" | "on" => Ok(true),
        _ => Err(AppError::Config(format!(
            "invalid {name} '{raw}': expected true/false"
        ))),
    }
}

impl Config {
    /// Load configuration from environment variables with sensible defaults.
    pub fn from_env() -> Result<Self, AppError> {
        let port_str = env::var("PORT")
            .or_else(|_| env::var("AWS_LWA_PORT"))
            .unwrap_or_else(|_| "8000".into());
        let port: u16 = port_str
            .parse()
            .map_err(|e| AppError::Config(format!("invalid port '{port_str}': {e}")))?;

        Ok(Self {
            environment: env::var("ENVIRONMENT").unwrap_or_else(|_| "dev".into()),
            app_name: env::var("APP_NAME").unwrap_or_else(|_| "supervictor".into()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
            port,
            store_backend: env::var("STORE_BACKEND").unwrap_or_else(|_| "sqlite".into()),
            devices_table: env::var("DEVICES_TABLE").unwrap_or_else(|_| "devices".into()),
            messages_table: env::var("MESSAGES_TABLE").unwrap_or_else(|_| "messages".into()),
            sqlite_db_path: env::var("SQLITE_DB_PATH").unwrap_or_else(|_| ":memory:".into()),
            watchdog_interval_secs: {
                let raw = env::var("WATCHDOG_INTERVAL_SECS").unwrap_or_else(|_| "300".into());
                raw.parse().map_err(|e| {
                    AppError::Config(format!("invalid WATCHDOG_INTERVAL_SECS '{raw}': {e}"))
                })?
            },
            uplink_ttl_days: {
                let raw = env::var("UPLINK_TTL_DAYS").unwrap_or_else(|_| "30".into());
                raw.parse().map_err(|e| {
                    AppError::Config(format!("invalid UPLINK_TTL_DAYS '{raw}': {e}"))
                })?
            },
            require_registration: {
                let raw = env::var("REQUIRE_REGISTRATION").unwrap_or_default();
                parse_bool("REQUIRE_REGISTRATION", &raw)?
            },
            tls: tls_from_env()?,
        })
    }
}
