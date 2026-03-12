use std::time::{Duration, Instant};

use crate::commands::ping::{build_mtls_agent, build_plain_agent};
use crate::env;
use crate::output;
use supervictor_common::routes as wire;

use super::{OnboardContext, PhaseResult};

const POLL_INTERVAL: u64 = 5;
const POLL_TIMEOUT: u64 = 60;

/// Poll the uplinks endpoint until the device sends its first reading.
pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    if ctx.dry_run {
        return PhaseResult::passed();
    }

    let (url, agent) = match resolve_verify(ctx) {
        Ok(v) => v,
        Err(e) => return PhaseResult::failed(e),
    };

    output::info(&format!("Polling {} (timeout {}s)", url, POLL_TIMEOUT));
    let deadline = Instant::now() + Duration::from_secs(POLL_TIMEOUT);
    let mut elapsed: u64 = 0;

    while Instant::now() < deadline {
        match agent.get(&url).call() {
            Ok(resp) => {
                let body = resp.into_body().read_to_string().unwrap_or_default();
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&body) {
                    if !arr.is_empty() {
                        return PhaseResult::passed();
                    }
                }
                output::info(&format!(
                    "  No uplinks yet ({}s / {}s)",
                    elapsed, POLL_TIMEOUT
                ));
            }
            Err(e) => {
                output::info(&format!(
                    "  Waiting for server ({}s / {}s): {}",
                    elapsed, POLL_TIMEOUT, e
                ));
            }
        }
        std::thread::sleep(Duration::from_secs(POLL_INTERVAL));
        elapsed += POLL_INTERVAL;
    }

    PhaseResult::failed(format!("No uplinks received within {}s", POLL_TIMEOUT))
}

fn resolve_verify(ctx: &OnboardContext) -> Result<(String, ureq::Agent), String> {
    let api_url = ctx.api_url.as_deref().unwrap_or("http://localhost:3000");
    let device_name = &ctx.device_name;

    let env_vars = env::load_env(&ctx.config.env_dev).unwrap_or_default();
    let host = env_vars.get("HOST").cloned().unwrap_or_default();

    if ctx.mode == "onprem" {
        if ctx.compose_file.is_some() {
            let agent = build_mtls_agent_from_ctx(ctx, true)?;
            let url = format!("{}{}/{}/uplinks", api_url, wire::DEVICES, device_name);
            return Ok((url, agent));
        }
        let port = env_vars.get("PORT").cloned().unwrap_or_default();
        let base = if !host.is_empty() && !port.is_empty() {
            format!("http://{}:{}", host, port)
        } else {
            api_url.to_string()
        };
        return Ok((
            format!("{}{}/{}/uplinks", base, wire::DEVICES, device_name),
            build_plain_agent(),
        ));
    }

    // AWS mode
    if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
        let url = format!("{}{}/{}/uplinks", api_url, wire::DEVICES, device_name);
        return Ok((url, build_plain_agent()));
    }

    // Remote host — use mTLS
    output::info(&format!("Device targets {} — verifying via mTLS", host));
    let agent = build_mtls_agent_from_ctx(ctx, false)?;
    let url = format!("https://{}{}/{}/uplinks", host, wire::DEVICES, device_name);
    Ok((url, agent))
}

fn build_mtls_agent_from_ctx(
    ctx: &OnboardContext,
    _use_local_ca: bool,
) -> Result<ureq::Agent, String> {
    let certs_dir = ctx
        .certs_dir
        .as_ref()
        .ok_or_else(|| "certs_dir not set".to_string())?;

    let cert = certs_dir
        .join("devices")
        .join(&ctx.device_name)
        .join("client.pem");
    let key = certs_dir
        .join("devices")
        .join(&ctx.device_name)
        .join("client.key");
    let ca = certs_dir.join("ca/ca.pem");

    build_mtls_agent(Some(&ca), &cert, &key).map_err(|e| format!("{}", e))
}
