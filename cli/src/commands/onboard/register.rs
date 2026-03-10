use crate::commands::ping::{build_mtls_agent, build_plain_agent};
use supervictor_wire::models::{DeviceResponse, RegisterDeviceRequest};
use supervictor_wire::{routes as wire, status};

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
    let register_url = format!("{}{}", api_url, wire::DEVICES);
    let payload = RegisterDeviceRequest {
        device_id: ctx.device_name.clone(),
        owner_id: ctx.owner_id.clone(),
        subject_dn: ctx.subject_dn.clone(),
    };
    let body = serde_json::to_string(&payload).unwrap();

    match agent
        .post(&register_url)
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
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
    let verify_url = format!("{}{}/{}", api_url, wire::DEVICES, ctx.device_name);
    match agent.get(&verify_url).call() {
        Ok(resp) => {
            let resp_body = resp.into_body().read_to_string().unwrap_or_default();
            match serde_json::from_str::<DeviceResponse>(&resp_body) {
                Ok(device) => {
                    if device.status != status::ACTIVE {
                        return PhaseResult::failed(format!(
                            "Device status is '{}', expected 'active'",
                            device.status
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

            return build_mtls_agent(Some(&ca), &cert, &key).map_err(|e| format!("{}", e));
        }
    }

    Ok(build_plain_agent())
}
