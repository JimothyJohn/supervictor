use std::fs;
use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::runner::RunOptions;
use crate::sam::SamLocal;

use super::{OnboardContext, PhaseResult};

const POLL_TIMEOUT: u64 = 60;
const POLL_INTERVAL: u64 = 2;

/// Start the API server (Docker Compose for onprem, SAM local for AWS).
pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    if ctx.mode == "onprem" {
        start_compose(ctx)
    } else {
        start_aws(ctx)
    }
}

// ── On-prem (Docker Compose) ─────────────────────────────────────────

fn detect_lan_ip() -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("UDP bind: {}", e))?;
    socket
        .connect("10.255.255.255:1")
        .map_err(|e| format!("UDP connect: {}", e))?;
    let addr = socket
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?;
    Ok(addr.ip().to_string())
}

fn ensure_server_cert(ctx: &OnboardContext, host_ip: &str) -> Result<(), String> {
    let certs_dir = ctx.config.repo_root.join("certs");
    let server_dir = certs_dir.join("servers/caddy");
    if server_dir.join("server.pem").exists() && server_dir.join("server.key").exists() {
        return Ok(());
    }

    let gen_script = ctx.config.gen_certs_script_path();
    let gen_str = gen_script.to_string_lossy().to_string();
    ctx.runner
        .run(
            &[&gen_str, "server", "caddy", host_ip],
            &RunOptions {
                verbose: ctx.verbose,
                dry_run: ctx.dry_run,
                ..Default::default()
            },
        )
        .map_err(|e| format!("Server cert generation failed: {}", e))?;
    Ok(())
}

fn start_compose(ctx: &mut OnboardContext) -> PhaseResult {
    let compose_file = ctx.config.endpoint_dir.join("docker-compose.yml");
    if !compose_file.exists() {
        return PhaseResult::failed(format!("Missing {}", compose_file.display()));
    }

    let host_ip = match detect_lan_ip() {
        Ok(ip) => ip,
        Err(e) => return PhaseResult::failed(format!("Cannot detect LAN IP: {}", e)),
    };

    if let Err(e) = ensure_server_cert(ctx, &host_ip) {
        return PhaseResult::failed(e);
    }

    let _ = fs::create_dir_all(&ctx.config.log_dir);
    let compose_str = compose_file.to_string_lossy().to_string();

    // Tear down pre-existing stack
    let _ = ctx.runner.run(
        &[
            "docker",
            "compose",
            "-f",
            &compose_str,
            "down",
            "--remove-orphans",
        ],
        &RunOptions {
            verbose: ctx.verbose,
            dry_run: ctx.dry_run,
            check: false,
            log_to: Some(ctx.config.log_dir.join("compose_down.log")),
            ..Default::default()
        },
    );

    // Start compose stack
    if let Err(e) = ctx.runner.run(
        &[
            "docker",
            "compose",
            "-f",
            &compose_str,
            "up",
            "-d",
            "--build",
        ],
        &RunOptions {
            verbose: ctx.verbose,
            dry_run: ctx.dry_run,
            log_to: Some(ctx.config.log_dir.join("compose_up.log")),
            ..Default::default()
        },
    ) {
        return PhaseResult::failed(format!("Compose up failed: {}", e));
    }

    ctx.compose_file = Some(compose_file);
    ctx.api_url = Some(format!("https://{}", host_ip));

    if ctx.dry_run {
        return PhaseResult::passed();
    }

    // Wait for HTTPS readiness (just TCP probe on 443)
    if !wait_for_tcp(&host_ip, 443, POLL_TIMEOUT) {
        return PhaseResult::failed("Compose stack did not become ready in time");
    }

    PhaseResult::passed()
}

fn wait_for_tcp(host: &str, port: u16, timeout_secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let addr = format!("{}:{}", host, port);
    while Instant::now() < deadline {
        if let Ok(parsed) = addr.parse() {
            if TcpStream::connect_timeout(&parsed, Duration::from_secs(2)).is_ok() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_secs(POLL_INTERVAL));
    }
    false
}

// ── AWS (SAM local) ──────────────────────────────────────────────────

fn start_aws(ctx: &mut OnboardContext) -> PhaseResult {
    let env_file = write_sam_env_overrides(ctx);
    let env_file_str = env_file.to_string_lossy().to_string();

    let mut sam = SamLocal::new(ctx.config, None, ctx.verbose, ctx.dry_run);

    if let Err(e) = sam.build(ctx.runner, false) {
        return PhaseResult::failed(format!("SAM build failed: {}", e));
    }

    match sam.start(ctx.runner, &["--env-vars", &env_file_str]) {
        Ok(mut guard) => {
            if let Err(e) = sam.wait_ready() {
                return PhaseResult::failed(format!("SAM local failed: {}", e));
            }
            ctx.api_url = Some(sam.url());
            // Transfer the child process to OnboardContext so it stays alive
            // across phases and gets cleaned up on drop
            ctx.api_process = guard.take_process();
            PhaseResult::passed()
        }
        Err(e) => PhaseResult::failed(format!("SAM local start failed: {}", e)),
    }
}

fn write_sam_env_overrides(ctx: &OnboardContext) -> PathBuf {
    let overrides =
        r#"{"EndpointFunction":{"STORE_BACKEND":"sqlite","SQLITE_DB_PATH":"/tmp/supervictor.db"}}"#;
    let dir = &ctx.config.log_dir;
    let _ = fs::create_dir_all(dir);
    let path = dir.join("sam_env_vars.json");
    let _ = fs::write(&path, overrides);
    path
}
