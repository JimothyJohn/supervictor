//! Runtime configuration: which endpoint, which admin identity.
//!
//! Everything comes from the environment so an MCP client config (Claude
//! Code's `.mcp.json`, Claude Desktop, an IDE) can point the server at any
//! deployment without flags:
//!
//! | Variable | Meaning | Default |
//! |---|---|---|
//! | `SUPERVICTOR_URL` | `https://host[:port][/prefix]` of the endpoint | `https://localhost:8443` |
//! | `SUPERVICTOR_CA` | CA bundle the server certificate chains to | `certs/ca/ca.pem` |
//! | `SUPERVICTOR_ADMIN_CERT` | admin client certificate (`OU=admin`) | `certs/admins/$USER/admin.pem` |
//! | `SUPERVICTOR_ADMIN_KEY` | its private key | `certs/admins/$USER/admin.key` |
//! | `SUPERVICTOR_TIMEOUT_SECS` | per-request budget (connect + TLS + response) | `10` |
//!
//! Relative paths resolve against the process working directory, which for
//! a project-scoped `.mcp.json` is the repository root — the layout
//! `./Quickstart certs` writes.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

/// Where one endpoint lives. Parsed from a URL; the path is kept as a
/// prefix so a raw API Gateway URL (`https://x.execute-api.../prod`) works
/// the same as a custom domain (`https://supervictor.example`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Hostname or IP literal (no brackets), also the TLS server name.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Path prefix without a trailing slash; empty for none.
    pub prefix: String,
}

impl Target {
    /// Parse `https://host[:port][/prefix]`. Plain `http://` is refused:
    /// every tool authenticates with a client certificate, which only a TLS
    /// connection can carry.
    pub fn parse(url: &str) -> Result<Self, String> {
        let url = url.trim();
        let rest = match url.strip_prefix("https://") {
            Some(rest) => rest,
            None if url.starts_with("http://") => {
                return Err(format!(
                    "{url}: plain http is not supported — the admin identity is a client \
                     certificate, so the endpoint must be reached over https"
                ))
            }
            None => {
                return Err(format!(
                    "{url}: expected https://host[:port][/prefix] (SUPERVICTOR_URL or the \
                     tool's `endpoint` argument)"
                ))
            }
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if authority.contains('@') || authority.contains('?') || authority.contains('#') {
            return Err(format!(
                "{url}: userinfo, query and fragment are not allowed"
            ));
        }
        let (host, port) =
            split_host_port(authority).ok_or_else(|| format!("{url}: malformed host or port"))?;
        if host.is_empty() {
            return Err(format!("{url}: empty host"));
        }
        if path.contains('?') || path.contains('#') {
            return Err(format!("{url}: query and fragment are not allowed"));
        }
        let prefix = path.trim_end_matches('/').to_string();
        Ok(Self {
            host: host.to_string(),
            port,
            prefix,
        })
    }

    /// Full request path for a route: prefix + route.
    pub fn path(&self, route: &str) -> String {
        format!("{}{}", self.prefix, route)
    }

    /// `host:port` as a dial string (IPv6 bracketed).
    pub fn dial(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// `Host:` header value: the authority with the port only when it is
    /// not the https default.
    pub fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 443 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    /// Human-readable URL for messages and results.
    pub fn display(&self) -> String {
        format!("https://{}{}", self.host_header(), self.prefix)
    }
}

/// `host`, `host:port`, `[v6]`, `[v6]:port` → (host, port); port defaults
/// to 443.
fn split_host_port(authority: &str) -> Option<(&str, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        let after = &rest[end + 1..];
        return match after.strip_prefix(':') {
            Some(port) => Some((host, port.parse().ok()?)),
            None if after.is_empty() => Some((host, 443)),
            None => None,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Some((host, port.parse().ok()?)),
        Some(_) => None, // bare IPv6 without brackets: ambiguous
        None => Some((authority, 443)),
    }
}

/// Everything the server needs, resolved once at startup.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default endpoint (tools may override per call).
    pub target: Target,
    /// CA bundle the server certificate must chain to.
    pub ca: PathBuf,
    /// Admin client certificate.
    pub cert: PathBuf,
    /// Admin private key.
    pub key: PathBuf,
    /// Per-request budget.
    pub timeout: Duration,
}

impl Config {
    /// Read `SUPERVICTOR_*` from the environment (see the module docs).
    pub fn from_env() -> Result<Self, String> {
        let get = |name: &str| env::var(name).ok().filter(|v| !v.trim().is_empty());
        let admin = get("SUPERVICTOR_ADMIN")
            .or_else(|| get("USER"))
            .unwrap_or_else(|| "admin".into());
        let target = Target::parse(
            &get("SUPERVICTOR_URL").unwrap_or_else(|| "https://localhost:8443".into()),
        )?;
        let timeout = match get("SUPERVICTOR_TIMEOUT_SECS") {
            Some(raw) => Duration::from_secs(raw.trim().parse::<u64>().map_err(|e| {
                format!("SUPERVICTOR_TIMEOUT_SECS '{raw}' is not a whole number of seconds: {e}")
            })?),
            None => Duration::from_secs(10),
        };
        if timeout.is_zero() {
            return Err("SUPERVICTOR_TIMEOUT_SECS must be at least 1".into());
        }
        Ok(Self {
            target,
            ca: get("SUPERVICTOR_CA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("certs/ca/ca.pem")),
            cert: get("SUPERVICTOR_ADMIN_CERT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("certs/admins/{admin}/admin.pem"))),
            key: get("SUPERVICTOR_ADMIN_KEY")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("certs/admins/{admin}/admin.key"))),
            timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_domain_default_port() {
        let t = Target::parse("https://supervictor.example").unwrap();
        assert_eq!(t.host, "supervictor.example");
        assert_eq!(t.port, 443);
        assert_eq!(t.prefix, "");
        assert_eq!(t.host_header(), "supervictor.example");
        assert_eq!(t.path("/fleet"), "/fleet");
        assert_eq!(t.display(), "https://supervictor.example");
    }

    #[test]
    fn parses_port_and_prefix() {
        let t = Target::parse("https://localhost:8443/").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.prefix.as_str()),
            ("localhost", 8443, "")
        );
        let t = Target::parse("https://abc.execute-api.us-east-1.amazonaws.com/prod/").unwrap();
        assert_eq!(t.prefix, "/prod");
        assert_eq!(t.path("/fleet/summary"), "/prod/fleet/summary");
        assert_eq!(t.host_header(), "abc.execute-api.us-east-1.amazonaws.com");
    }

    #[test]
    fn parses_ipv6_literal() {
        let t = Target::parse("https://[::1]:8443").unwrap();
        assert_eq!(t.host, "::1");
        assert_eq!(t.dial(), "[::1]:8443");
        assert_eq!(t.host_header(), "[::1]:8443");
        let t = Target::parse("https://[::1]").unwrap();
        assert_eq!(t.port, 443);
    }

    #[test]
    fn rejects_plain_http_and_junk() {
        for bad in [
            "http://localhost:8443",
            "localhost:8443",
            "https://",
            "https://:8443",
            "https://host:notaport",
            "https://host:70000",
            "https://user@host",
            "https://host/fleet?x=1",
            "https://host#frag",
            "https://[::1",
            "https://[::1]x",
            "https://::1",
        ] {
            assert!(Target::parse(bad).is_err(), "{bad} should be rejected");
        }
        let err = Target::parse("http://localhost").unwrap_err();
        assert!(err.contains("https"), "{err}");
    }
}
