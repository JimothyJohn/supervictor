use crate::preflight::{check_docker_running, check_tools};

use super::{OnboardContext, PhaseResult};

const BASE_TOOLS: &[&str] = &["openssl", "cargo", "espflash", "docker"];
const AWS_TOOLS: &[&str] = &["sam", "aws"];

pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    let mut required: Vec<&str> = BASE_TOOLS.to_vec();
    if ctx.mode == "aws" {
        required.extend_from_slice(AWS_TOOLS);
    }

    let missing = check_tools(&required);
    if !missing.is_empty() {
        return PhaseResult::failed(format!("Missing tools: {}", missing.join(", ")));
    }

    if !check_docker_running(ctx.runner) {
        return PhaseResult::failed("Docker daemon is not running");
    }

    if !ctx.config.env_dev.exists() {
        return PhaseResult::failed(format!(
            ".env.dev not found at {}",
            ctx.config.env_dev.display()
        ));
    }

    PhaseResult::passed()
}
