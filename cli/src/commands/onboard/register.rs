use crate::commands::ping::{build_mtls_agent, build_plain_agent};

use super::{OnboardContext, PhaseResult};

pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    if ctx.dry_run {
        return PhaseResult::passed();
    }

    let api_url = match &ctx.api_url {
        Some(url) => url.clone(),
        None => return PhaseResult::failed("api_url not set"),
    };

    let agent = match build_agent(ctx) {
        Ok(a) => a,
        Err(e) => return PhaseResult::failed(format!("TLS setup failed: {}", e)),
    };

    // POST registration
    let register_url = format!("{}/devices", api_url);
    let payload = serde_json::json!({
        "device_id": ctx.device_name,
        "owner_id": ctx.owner_id,
        "subject_dn": ctx.subject_dn,
    });

    match agent
        .post(&register_url)
        .header("Content-Type", "application/json")
        .send(payload.to_string().as_bytes())
    {
        Ok(resp) => {
            if resp.status() != 201 {
                return PhaseResult::failed(format!(
                    "Registration returned HTTP {}, expected 201",
                    resp.status()
                ));
            }
        }
        Err(ureq::Error::StatusCode(code)) => {
            return PhaseResult::failed(format!("Registration failed: HTTP {}", code));
        }
        Err(e) => {
            return PhaseResult::failed(format!("Cannot reach API: {}", e));
        }
    }

    // GET to verify device is active
    let verify_url = format!("{}/devices/{}", api_url, ctx.device_name);
    match agent.get(&verify_url).call() {
        Ok(resp) => {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(json) => {
                    if json.get("status").and_then(|s| s.as_str()) != Some("active") {
                        return PhaseResult::failed(format!(
                            "Device status is '{}', expected 'active'",
                            json.get("status")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                        ));
                    }
                }
                Err(e) => {
                    return PhaseResult::failed(format!("Failed to parse response: {}", e));
                }
            }
        }
        Err(e) => {
            return PhaseResult::failed(format!("Verification GET failed: {}", e));
        }
    }

    PhaseResult::passed()
}

fn build_agent(ctx: &OnboardContext) -> Result<ureq::Agent, String> {
    if ctx.compose_file.is_some() {
        if let Some(ref certs_dir) = ctx.certs_dir {
            let ca = certs_dir.join("ca/ca.pem");
            let cert = certs_dir
                .join("devices")
                .join(&ctx.device_name)
                .join("client.pem");
            let key = certs_dir
                .join("devices")
                .join(&ctx.device_name)
                .join("client.key");

            return build_mtls_agent(&ca, &cert, &key).map_err(|e| format!("{}", e));
        }
    }

    Ok(build_plain_agent())
}
