use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::preflight;
use crate::runner::{self, RunOptions, Runner};
use crate::rust_tools;
use crate::sam::{self, SamLocal};

/// Arguments for the `qs dev` command.
pub struct DevArgs {
    /// Enable verbose output.
    pub verbose: bool,
    /// Print commands without executing.
    pub dry_run: bool,
    /// Detach SAM local so it keeps running after the pipeline.
    pub serve: bool,
    /// Stop a previously-detached SAM local process.
    pub stop: bool,
}

/// Run the local development pipeline: tests, SAM build, and optional serve.
pub fn run_dev(args: &DevArgs, config: &ProjectConfig, r: &dyn Runner) -> Result<i32, CliError> {
    // Handle --stop: kill any running SAM process and exit
    if args.stop {
        return stop_server(config);
    }

    // Load env
    runner::step("Loading .env.dev");
    let env_vars = env::load_env(&config.env_dev)?;
    let env = env::make_env(&env_vars);

    // Override SAM port if set in env
    let mut cfg = config.clone();
    if let Some(port_str) = env_vars.get("SAM_LOCAL_PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            cfg.sam_local_port = port;
        }
    }

    preflight::require(&["sam", "docker", "cargo"], true, r)?;

    let log_dir = &cfg.log_dir;

    // Rust library tests
    runner::step("Running Rust library tests");
    let host_target = rust_tools::host_target(r)?;
    if r.run(
        &["cargo", "test", "--lib", "--target", &host_target],
        &RunOptions {
            cwd: Some(cfg.device_dir.clone()),
            env: Some(env.clone()),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("rust_tests.log")),
            ..Default::default()
        },
    )
    .is_err()
    {
        runner::error(&format!(
            "Rust library tests failed (see {})",
            log_dir.join("rust_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Rust library tests passed");

    // Endpoint tests
    runner::step("Running endpoint tests");
    if r.run(
        &["cargo", "test", "--features", "sqlite"],
        &RunOptions {
            cwd: Some(cfg.endpoint_dir.clone()),
            env: Some(env.clone()),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("endpoint_tests.log")),
            ..Default::default()
        },
    )
    .is_err()
    {
        runner::error(&format!(
            "Endpoint tests failed (see {})",
            log_dir.join("endpoint_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Endpoint tests passed");

    // SAM build
    let mut sam = SamLocal::new(&cfg, Some(env.clone()), args.verbose, args.dry_run);
    sam.build(r, false)?;

    // Start sam local
    let guard = sam.start(r, &[])?;
    sam.wait_ready()?;

    if args.serve {
        // Stop any previously-running SAM server first
        let _ = sam::stop_from_pid_file(&cfg.sam_pid_file);

        let url = sam.url();
        if args.dry_run {
            println!("  [dry-run] detach SAM process");
        } else {
            let pid = guard.detach(&cfg.sam_pid_file)?;
            runner::success(&format!("sam local running (PID {}) at {}", pid, url));
        }

        println!("  Logs: {}", cfg.sam_log_file.display());
        println!("  Stop: qs dev --stop");
    } else {
        runner::step("Running local integration tests");
        let mut test_vars = env_vars.clone();
        test_vars.insert("DEPLOYED_URL".to_string(), sam.url());
        let test_env = env::make_env(&test_vars);

        if r.run(
            &["cargo", "test", "--features", "sqlite", "--", "--ignored"],
            &RunOptions {
                cwd: Some(cfg.endpoint_dir.clone()),
                env: Some(test_env),
                verbose: args.verbose,
                dry_run: args.dry_run,
                log_to: Some(log_dir.join("integration_tests.log")),
                ..Default::default()
            },
        )
        .is_err()
        {
            runner::error(&format!(
                "Integration tests failed (see {})",
                log_dir.join("integration_tests.log").display()
            ));
            return Ok(1);
        }
        runner::success("Integration tests passed");
    }

    runner::success("\nDev pipeline passed.");
    Ok(0)
}

fn stop_server(config: &ProjectConfig) -> Result<i32, CliError> {
    match sam::stop_from_pid_file(&config.sam_pid_file)? {
        true => {
            runner::success("Stopped sam local.");
            Ok(0)
        }
        false => {
            runner::error("No running sam local found.");
            Ok(1)
        }
    }
}

#[cfg(test)]
#[path = "dev_tests.rs"]
mod tests;
