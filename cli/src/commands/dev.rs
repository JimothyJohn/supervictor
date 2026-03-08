use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::preflight;
use crate::runner::{self, RunOptions, Runner};
use crate::rust_tools;
use crate::sam::SamLocal;

pub struct DevArgs {
    pub verbose: bool,
    pub dry_run: bool,
    pub serve: bool,
}

pub fn run_dev(args: &DevArgs, config: &ProjectConfig, r: &dyn Runner) -> Result<i32, CliError> {
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

    preflight::require(&["uv", "sam", "docker", "cargo"], true, r)?;

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
    ).is_err() {
        runner::error(&format!(
            "Rust library tests failed (see {})",
            log_dir.join("rust_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Rust library tests passed");

    // Python unit tests
    runner::step("Running Python unit tests");
    if r.run(
        &["uv", "run", "pytest", "tests/unit/", "-v"],
        &RunOptions {
            cwd: Some(cfg.cloud_dir.clone()),
            env: Some(env.clone()),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("python_unit_tests.log")),
            ..Default::default()
        },
    ).is_err() {
        runner::error(&format!(
            "Python unit tests failed (see {})",
            log_dir.join("python_unit_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Python unit tests passed");

    // SAM build
    let mut sam = SamLocal::new(&cfg, Some(env.clone()), args.verbose, args.dry_run);
    sam.build(r, false)?;

    // Start sam local
    let mut guard = sam.start(r, &[])?;
    sam.wait_ready()?;

    if args.serve {
        let url = sam.url();
        println!("\n  sam local running at {}", url);
        println!("  GET  {}/hello", url);
        println!(
            "  POST {}/hello  -d '{{\"id\":\"test\",\"current\":42}}'",
            url
        );
        println!("\n  Press Ctrl+C to stop.");
        guard.wait()?;
    } else {
        runner::step("Running local integration tests");
        let mut test_vars = env_vars.clone();
        test_vars.insert("SAM_LOCAL_URL".to_string(), sam.url());
        let test_env = env::make_env(&test_vars);

        if r.run(
            &[
                "uv",
                "run",
                "pytest",
                "tests/integration/",
                "-m",
                "local",
                "-v",
            ],
            &RunOptions {
                cwd: Some(cfg.cloud_dir.clone()),
                env: Some(test_env),
                verbose: args.verbose,
                dry_run: args.dry_run,
                log_to: Some(log_dir.join("integration_tests.log")),
                ..Default::default()
            },
        ).is_err() {
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

#[cfg(test)]
#[path = "dev_tests.rs"]
mod tests;
