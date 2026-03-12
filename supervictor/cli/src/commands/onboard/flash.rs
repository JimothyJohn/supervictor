use crate::env;
use crate::runner::RunOptions;

use super::{OnboardContext, PhaseResult};

/// Build the embedded firmware and flash it to the connected ESP32-C3.
pub fn run(ctx: &mut OnboardContext) -> PhaseResult {
    let mut env_vars = match env::load_env(&ctx.config.env_dev) {
        Ok(v) => v,
        Err(e) => return PhaseResult::failed(format!("Failed to load .env.dev: {}", e)),
    };
    env_vars.insert("DEVICE_NAME".to_string(), ctx.device_name.clone());
    let env = env::make_env(&env_vars);

    let opts = RunOptions {
        cwd: Some(ctx.config.edge_dir.clone()),
        env: Some(env),
        verbose: ctx.verbose,
        dry_run: ctx.dry_run,
        ..Default::default()
    };

    // Build
    if let Err(e) = ctx.runner.run(
        &[
            "cargo",
            "build",
            "--bin",
            "supervictor-embedded",
            "--features",
            "embedded",
        ],
        &opts,
    ) {
        return PhaseResult::failed(format!("Build failed: {}", e));
    }

    // Flash
    if let Err(e) = ctx.runner.run(
        &[
            "espflash",
            "flash",
            "--chip",
            "esp32c3",
            "target/riscv32imc-unknown-none-elf/debug/supervictor-embedded",
        ],
        &opts,
    ) {
        return PhaseResult::failed(format!("Flash failed: {}", e));
    }

    PhaseResult::passed()
}
