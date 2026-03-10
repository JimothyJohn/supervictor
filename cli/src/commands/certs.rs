use crate::config::ProjectConfig;
use crate::error::CliError;
use crate::preflight;
use crate::runner::{self, RunOptions, Runner};

/// Arguments for the `qs certs` command.
pub struct CertsArgs {
    /// Enable verbose output.
    pub verbose: bool,
    /// Print commands without executing.
    pub dry_run: bool,
    /// The certificate subcommand to execute.
    pub command: CertsCommand,
}

/// Certificate management subcommands.
pub enum CertsCommand {
    /// Generate a new root CA.
    Ca,
    /// Generate a device client certificate.
    Device {
        /// Device identity name (used as directory and CN).
        name: String,
        /// Optional validity period in days.
        days: Option<u32>,
    },
    /// Generate a server certificate with SAN.
    Server {
        /// Server name (used as directory).
        name: String,
        /// IP or hostname for the SAN extension.
        host_ip: String,
        /// Optional validity period in days.
        days: Option<u32>,
    },
    /// List all generated certificates.
    List,
    /// Verify a device and server cert chain against the CA.
    Verify {
        /// Device whose client cert to verify.
        device_name: String,
        /// Server whose cert to verify.
        server_name: String,
    },
    /// Test a live TLS handshake against a remote server.
    Handshake {
        /// Target hostname.
        host: String,
        /// Target port.
        port: String,
        /// Device name whose client cert to present.
        device_name: String,
        /// Optional TLS version flag (e.g. "tls1_2").
        tls_version: Option<String>,
        /// Also test connecting without a client cert.
        test_no_client: bool,
    },
}

/// Dispatch the `qs certs` subcommand.
pub fn run_certs(
    args: &CertsArgs,
    config: &ProjectConfig,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    preflight::require(&["openssl"], false, r)?;

    match &args.command {
        CertsCommand::Ca => run_gen(config, &["ca"], args.verbose, args.dry_run, r),
        CertsCommand::Device { name, days } => {
            let mut gen_args = vec!["device", name.as_str()];
            let days_str;
            if let Some(d) = days {
                days_str = d.to_string();
                gen_args.push(&days_str);
            }
            run_gen(config, &gen_args, args.verbose, args.dry_run, r)
        }
        CertsCommand::Server {
            name,
            host_ip,
            days,
        } => {
            let mut gen_args = vec!["server", name.as_str(), host_ip.as_str()];
            let days_str;
            if let Some(d) = days {
                days_str = d.to_string();
                gen_args.push(&days_str);
            }
            run_gen(config, &gen_args, args.verbose, args.dry_run, r)
        }
        CertsCommand::List => run_gen(config, &["list"], args.verbose, args.dry_run, r),
        CertsCommand::Verify {
            device_name,
            server_name,
        } => cmd_verify(
            config,
            device_name,
            server_name,
            args.verbose,
            args.dry_run,
            r,
        ),
        CertsCommand::Handshake {
            host,
            port,
            device_name,
            tls_version,
            test_no_client,
        } => cmd_handshake(
            config,
            &HandshakeArgs {
                host,
                port,
                device_name,
                tls_version: tls_version.as_deref(),
                test_no_client: *test_no_client,
                verbose: args.verbose,
                dry_run: args.dry_run,
            },
            r,
        ),
    }
}

fn run_gen(
    config: &ProjectConfig,
    gen_args: &[&str],
    verbose: bool,
    dry_run: bool,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    let script = config.gen_certs_script_path();
    let script_str = script.to_string_lossy().to_string();
    let mut cmd: Vec<&str> = vec![&script_str];
    cmd.extend_from_slice(gen_args);

    match r.run(
        &cmd,
        &RunOptions {
            cwd: Some(config.repo_root.clone()),
            verbose,
            dry_run,
            ..Default::default()
        },
    ) {
        Ok(_) => Ok(0),
        Err(e) => {
            runner::error(&format!(
                "gen_certs.sh {} failed: {}",
                gen_args.join(" "),
                e
            ));
            Ok(1)
        }
    }
}

fn cmd_verify(
    config: &ProjectConfig,
    device_name: &str,
    server_name: &str,
    verbose: bool,
    dry_run: bool,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    let certs = config.certs_dir();
    let ca_pem = certs.join("ca/ca.pem");
    let server_pem = certs.join(format!("servers/{}/server.pem", server_name));
    let client_pem = certs.join(format!("devices/{}/client.pem", device_name));
    let ca_str = ca_pem.to_string_lossy().to_string();
    let server_str = server_pem.to_string_lossy().to_string();
    let client_str = client_pem.to_string_lossy().to_string();

    let opts = RunOptions {
        capture: true,
        verbose,
        dry_run,
        ..Default::default()
    };

    let mut all_ok = true;

    // 1. Verify CA
    runner::step("Verify root CA");
    match r.run(&["openssl", "verify", "-CAfile", &ca_str, &ca_str], &opts) {
        Ok(result) if dry_run || result.stdout.contains("OK") => {
            runner::success(&format!("  {}", result.stdout.trim()));
        }
        Ok(result) => {
            runner::error(&format!("  FAIL: {}", result.stdout.trim()));
            all_ok = false;
        }
        Err(e) => {
            runner::error(&format!("  FAIL: {}", e));
            all_ok = false;
        }
    }

    // 2. Verify server cert
    runner::step("Verify server cert against CA");
    match r.run(
        &["openssl", "verify", "-CAfile", &ca_str, &server_str],
        &opts,
    ) {
        Ok(result) if dry_run || result.stdout.contains("OK") => {
            runner::success(&format!("  {}", result.stdout.trim()));
        }
        Ok(result) => {
            runner::error(&format!("  FAIL: {}", result.stdout.trim()));
            all_ok = false;
        }
        Err(e) => {
            runner::error(&format!("  FAIL: {}", e));
            all_ok = false;
        }
    }

    // 3. Verify client cert
    runner::step("Verify client cert against CA");
    match r.run(
        &["openssl", "verify", "-CAfile", &ca_str, &client_str],
        &opts,
    ) {
        Ok(result) if dry_run || result.stdout.contains("OK") => {
            runner::success(&format!("  {}", result.stdout.trim()));
        }
        Ok(result) => {
            runner::error(&format!("  FAIL: {}", result.stdout.trim()));
            all_ok = false;
        }
        Err(e) => {
            runner::error(&format!("  FAIL: {}", e));
            all_ok = false;
        }
    }

    // 4. Server cert SAN
    runner::step("Server cert SAN");
    if let Ok(result) = r.run(
        &[
            "openssl",
            "x509",
            "-in",
            &server_str,
            "-noout",
            "-ext",
            "subjectAltName",
        ],
        &RunOptions {
            check: false,
            ..opts.clone()
        },
    ) {
        runner::success(&format!("  {}", result.stdout.trim()));
    }

    // 5. Client cert subject
    runner::step("Client cert subject");
    if let Ok(result) = r.run(
        &["openssl", "x509", "-in", &client_str, "-noout", "-subject"],
        &RunOptions {
            check: false,
            ..opts.clone()
        },
    ) {
        runner::success(&format!("  {}", result.stdout.trim()));
    }

    // 6. Client cert Extended Key Usage
    runner::step("Client cert Extended Key Usage");
    match r.run(
        &[
            "openssl",
            "x509",
            "-in",
            &client_str,
            "-noout",
            "-ext",
            "extendedKeyUsage",
        ],
        &RunOptions {
            check: false,
            ..opts.clone()
        },
    ) {
        Ok(result) if !dry_run => {
            if result.stdout.contains("clientAuth") {
                runner::success(&format!("  {}", result.stdout.trim()));
            } else {
                runner::error(&format!(
                    "  FAIL: clientAuth not found in Extended Key Usage: {}",
                    result.stdout.trim()
                ));
                all_ok = false;
            }
        }
        Ok(result) => runner::success(&format!("  {}", result.stdout.trim())),
        Err(e) => {
            runner::error(&format!("  FAIL: could not read Extended Key Usage: {}", e));
            all_ok = false;
        }
    }

    // 7. Expiry dates
    runner::step("Certificate expiry dates");
    for (label, cert_path) in [
        ("CA", &ca_str),
        ("Server", &server_str),
        ("Client", &client_str),
    ] {
        match r.run(
            &["openssl", "x509", "-in", cert_path, "-noout", "-enddate"],
            &RunOptions {
                check: false,
                ..opts.clone()
            },
        ) {
            Ok(result) if !dry_run => {
                let expiry = result
                    .stdout
                    .trim()
                    .strip_prefix("notAfter=")
                    .unwrap_or(result.stdout.trim());
                runner::success(&format!("  {}: expires {}", label, expiry));
            }
            Ok(_) => {}
            Err(e) => {
                runner::error(&format!("  {} expiry check failed: {}", label, e));
                all_ok = false;
            }
        }
    }

    if all_ok {
        runner::success("\nAll checks passed.");
        Ok(0)
    } else {
        runner::error("\nSome checks failed.");
        Ok(1)
    }
}

struct HandshakeArgs<'a> {
    host: &'a str,
    port: &'a str,
    device_name: &'a str,
    tls_version: Option<&'a str>,
    test_no_client: bool,
    verbose: bool,
    dry_run: bool,
}

fn cmd_handshake(
    config: &ProjectConfig,
    ha: &HandshakeArgs<'_>,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    let host = ha.host;
    let port = ha.port;
    let device_name = ha.device_name;
    let tls_version = ha.tls_version;
    let test_no_client = ha.test_no_client;
    let verbose = ha.verbose;
    let dry_run = ha.dry_run;
    let certs = config.certs_dir();
    let ca_pem = certs.join("ca/ca.pem");
    let client_pem = certs.join(format!("devices/{}/client.pem", device_name));
    let client_key = certs.join(format!("devices/{}/client.key", device_name));
    let ca_str = ca_pem.to_string_lossy().to_string();
    let client_pem_str = client_pem.to_string_lossy().to_string();
    let client_key_str = client_key.to_string_lossy().to_string();
    let target = format!("{}:{}", host, port);

    let opts = RunOptions {
        capture: true,
        verbose,
        dry_run,
        check: false,
        ..Default::default()
    };

    let mut all_ok = true;

    // 1. Full mTLS handshake
    runner::step(&format!("mTLS handshake to {}", target));
    let mut mtls_cmd = vec![
        "openssl",
        "s_client",
        "-connect",
        &target,
        "-cert",
        &client_pem_str,
        "-key",
        &client_key_str,
        "-CAfile",
        &ca_str,
    ];
    let tls_flag = tls_version.map(|ver| format!("-{}", ver));
    if let Some(ref flag) = tls_flag {
        mtls_cmd.push(flag);
    }

    match r.run(&mtls_cmd, &opts) {
        Ok(result) if !dry_run => {
            if result.stdout.contains("Verify return code: 0 (ok)") {
                runner::success("  Handshake OK — Verify return code: 0 (ok)");
            } else {
                let diag = result
                    .stdout
                    .lines()
                    .find(|l| l.contains("Verify return code:"))
                    .unwrap_or("could not find verify return code in output");
                runner::error(&format!("  FAIL: {}", diag.trim()));
                all_ok = false;
            }
        }
        Ok(_) => {}
        Err(e) => {
            runner::error(&format!("  FAIL: mTLS handshake failed: {}", e));
            all_ok = false;
        }
    }

    // 2. Without client cert
    if test_no_client {
        runner::step(&format!(
            "Connecting without client cert to {} (should fail if mTLS enforced)",
            target
        ));
        let mut no_client_cmd = vec![
            "openssl", "s_client", "-connect", &target, "-CAfile", &ca_str,
        ];
        if let Some(ref flag) = tls_flag {
            no_client_cmd.push(flag);
        }

        match r.run(&no_client_cmd, &opts) {
            Ok(result) if !dry_run => {
                if result.stdout.contains("Verify return code: 0 (ok)") {
                    runner::error(
                        "  WARN: server accepted connection without client cert — mTLS may not be enforced",
                    );
                } else {
                    runner::success(
                        "  Server rejected connection without client cert (mTLS enforced)",
                    );
                }
            }
            Ok(_) => {}
            Err(_) => {
                runner::success("  Server rejected connection without client cert (mTLS enforced)");
            }
        }
    }

    if all_ok {
        runner::success("\nHandshake checks passed.");
        Ok(0)
    } else {
        runner::error("\nHandshake checks failed.");
        Ok(1)
    }
}
