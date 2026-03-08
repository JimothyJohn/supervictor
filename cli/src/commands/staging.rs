use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::preflight;
use crate::runner::{self, RunOptions, Runner};
use crate::rust_tools;
use crate::sam::SamLocal;

use super::dev;

pub struct StagingArgs {
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn run_staging(
    args: &StagingArgs,
    config: &ProjectConfig,
    r: &dyn Runner,
    skip_dev_gate: bool,
) -> Result<i32, CliError> {
    // Gate: run full dev pipeline first
    if !skip_dev_gate {
        runner::step("Running dev gate");
        let dev_args = dev::DevArgs {
            verbose: args.verbose,
            dry_run: args.dry_run,
            serve: false,
        };
        let rc = dev::run_dev(&dev_args, config, r)?;
        if rc != 0 {
            runner::error("Dev pipeline failed. Aborting staging.");
            return Ok(rc);
        }
    }

    // Load staging env
    runner::step("Loading .env.staging");
    let staging_vars = env::load_env(&config.env_staging)?;
    let env = env::make_env(&staging_vars);

    preflight::require(&["uv", "sam", "docker", "openssl"], true, r)?;

    // Deploy to dev stack
    let sam = SamLocal::new(config, Some(env.clone()), args.verbose, args.dry_run);
    sam.build(r, false)?;
    sam.deploy(r, &config.sam_config_env_dev, false)?;

    // Run integration tests against deployed dev stack
    runner::step("Running integration tests against deployed dev stack");
    let sam_local_url = sam.stack_endpoint(r, &config.sam_config_env_dev)?;

    let mut test_vars = staging_vars.clone();
    test_vars.insert("SAM_LOCAL_URL".to_string(), sam_local_url.clone());
    let test_env = env::make_env(&test_vars);

    let log_dir = &config.log_dir;
    if let Err(_) = r.run(
        &[
            "uv", "run", "pytest", "tests/integration/", "-m", "local", "-v",
        ],
        &RunOptions {
            cwd: Some(config.cloud_dir.clone()),
            env: Some(test_env),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("staging_integration_tests.log")),
            ..Default::default()
        },
    ) {
        runner::error(&format!(
            "Staging integration tests failed (see {})",
            log_dir.join("staging_integration_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Staging integration tests passed");

    // Rust device integration tests
    runner::step("Running Rust device integration tests against deployed stack");
    let rust_target = rust_tools::host_target(r)?;
    let mut device_vars = staging_vars.clone();
    device_vars.insert("DEPLOYED_URL".to_string(), sam_local_url);
    let device_env = env::make_env(&device_vars);

    if let Err(_) = r.run(
        &[
            "cargo",
            "test",
            "--test",
            "deployed_roundtrip",
            "--target",
            &rust_target,
        ],
        &RunOptions {
            cwd: Some(config.device_dir.clone()),
            env: Some(device_env),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("device_deployed_tests.log")),
            ..Default::default()
        },
    ) {
        runner::error(&format!(
            "Rust device integration tests failed (see {})",
            log_dir.join("device_deployed_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("Rust device integration tests passed");

    // Verify mTLS against prod endpoint
    runner::step("Verifying mTLS against production endpoint");
    ensure_certs(config, &env, args.verbose, args.dry_run, r)?;

    let certs_dir_str = config.certs_dir().to_string_lossy().to_string();
    let mut mtls_vars = staging_vars.clone();
    mtls_vars.insert("API_ENDPOINT".to_string(), config.prod_api_endpoint.clone());
    mtls_vars.insert("TEST_CERT_DIR".to_string(), certs_dir_str);
    let mtls_env = env::make_env(&mtls_vars);

    if let Err(_) = r.run(
        &[
            "uv", "run", "pytest", "tests/integration/", "-m", "remote", "-v",
        ],
        &RunOptions {
            cwd: Some(config.cloud_dir.clone()),
            env: Some(mtls_env),
            verbose: args.verbose,
            dry_run: args.dry_run,
            log_to: Some(log_dir.join("mtls_tests.log")),
            ..Default::default()
        },
    ) {
        runner::error(&format!(
            "mTLS verification failed (see {})",
            log_dir.join("mtls_tests.log").display()
        ));
        return Ok(1);
    }
    runner::success("mTLS verification passed");

    runner::success("\nStaging pipeline passed.");
    Ok(0)
}

/// Generate test CA + device cert if missing.
fn ensure_certs(
    config: &ProjectConfig,
    env: &std::collections::HashMap<String, String>,
    verbose: bool,
    dry_run: bool,
    r: &dyn Runner,
) -> Result<(), CliError> {
    let certs_dir = config.certs_dir();
    let gen_script = config.gen_certs_script_path();
    let gen_script_str = gen_script.to_string_lossy().to_string();

    let opts = RunOptions {
        cwd: Some(config.cloud_dir.clone()),
        env: Some(env.clone()),
        verbose,
        dry_run,
        ..Default::default()
    };

    if !certs_dir.join("ca/ca.pem").exists() {
        runner::step("Generating test CA");
        r.run(&[&gen_script_str, "ca"], &opts)?;
    }

    if !certs_dir.join("devices/test-device/client.pem").exists() {
        runner::step("Generating test-device certificate");
        r.run(&[&gen_script_str, "device", "test-device"], &opts)?;
    }

    Ok(())
}
