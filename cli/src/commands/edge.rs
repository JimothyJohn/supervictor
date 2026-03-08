use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::preflight;
use crate::runner::{self, RunOptions, Runner};

pub struct EdgeArgs {
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn run_edge(
    args: &EdgeArgs,
    config: &ProjectConfig,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    preflight::require(&["cargo", "espflash"], false, r)?;

    runner::step("Loading .env.dev");
    let env_vars = env::load_env(&config.env_dev)?;
    let env = env::make_env(&env_vars);

    runner::milestone("Building and flashing embedded firmware");

    // .env.dev takes priority, fall back to OS environment
    let port = env_vars
        .get("ESPFLASH_PORT")
        .cloned()
        .or_else(|| std::env::var("ESPFLASH_PORT").ok())
        .unwrap_or_default();

    if !port.is_empty() {
        runner::step(&format!("Using serial port {}", port));
    }

    let mut cmd: Vec<&str> = vec![
        "cargo",
        "run",
        "--bin",
        "supervictor-embedded",
        "--features",
        "embedded",
    ];
    if !port.is_empty() {
        cmd.extend(["--", "--port", &port]);
    }

    match r.run(
        &cmd,
        &RunOptions {
            cwd: Some(config.device_dir.clone()),
            env: Some(env),
            verbose: args.verbose,
            dry_run: args.dry_run,
            ..Default::default()
        },
    ) {
        Ok(_) => Ok(0),
        Err(_) => {
            runner::error("Flash failed.");
            Ok(1)
        }
    }
}
