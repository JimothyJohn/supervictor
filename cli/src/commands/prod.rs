use std::process::Command;
use std::time::Duration;

use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::runner::{self, Runner};
use crate::sam::SamLocal;

use super::{dev, staging};

const TRUSTSTORE_DOMAIN: &str = "supervictor.advin.io";
const TRUSTSTORE_BUCKET: &str = "supervictor";
const TRUSTSTORE_KEY: &str = "truststore.pem";
const TRUSTSTORE_TEMP_KEY: &str = "truststore-reload.pem";

pub struct ProdArgs {
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn run_prod(
    args: &ProdArgs,
    config: &ProjectConfig,
    r: &dyn Runner,
) -> Result<i32, CliError> {
    // Gate 1: dev
    runner::milestone("Running dev gate");
    let dev_args = dev::DevArgs {
        verbose: args.verbose,
        dry_run: args.dry_run,
        serve: false,
    };
    let rc = dev::run_dev(&dev_args, config, r)?;
    if rc != 0 {
        runner::error("Dev pipeline failed. Aborting prod deployment.");
        return Ok(rc);
    }

    // Gate 2: staging (skip dev gate)
    runner::milestone("Running staging gate");
    let staging_args = staging::StagingArgs {
        verbose: args.verbose,
        dry_run: args.dry_run,
    };
    let rc = staging::run_staging(&staging_args, config, r, true)?;
    if rc != 0 {
        runner::error("Staging pipeline failed. Aborting prod deployment.");
        return Ok(rc);
    }

    // Confirmation
    println!();
    if !runner::confirm("All tests passed. Deploy to PRODUCTION? [y/N] ") {
        println!("Aborted.");
        return Ok(1);
    }

    // Deploy to prod
    runner::step("Loading .env.prod");
    let prod_vars = env::load_env(&config.env_prod)?;
    let env = env::make_env(&prod_vars);

    let sam = SamLocal::new(config, Some(env), args.verbose, args.dry_run);
    sam.build(r, true)?;
    let deployed = sam.deploy(r, &config.sam_config_env_prod, true)?;

    // Reload truststore
    reload_truststore(args.verbose, args.dry_run)?;

    if deployed {
        runner::success("\nProduction deployment complete.");
    } else {
        runner::success("\nNothing to deploy. Production stack is up to date.");
    }
    Ok(0)
}

fn reload_truststore(_verbose: bool, dry_run: bool) -> Result<(), CliError> {
    runner::step("Reloading API Gateway mTLS truststore");
    if dry_run {
        println!("  [dry-run] truststore reload skipped");
        return Ok(());
    }

    let uri = format!("s3://{}/{}", TRUSTSTORE_BUCKET, TRUSTSTORE_KEY);
    let temp_uri = format!("s3://{}/{}", TRUSTSTORE_BUCKET, TRUSTSTORE_TEMP_KEY);

    // Copy truststore to temp key
    let cp = aws_run(&["aws", "s3", "cp", &uri, &temp_uri], 0);
    if !cp.status.success() {
        runner::error(&format!(
            "Truststore copy failed: {}",
            String::from_utf8_lossy(&cp.stderr).trim()
        ));
        return Ok(());
    }

    let patch_temp = format!(
        "op=replace,path=/mutualTlsAuthentication/truststoreUri,value={}",
        temp_uri
    );
    let patch_canonical = format!(
        "op=replace,path=/mutualTlsAuthentication/truststoreUri,value={}",
        uri
    );

    // Point domain to temp URI
    let swap = aws_run(
        &[
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            TRUSTSTORE_DOMAIN,
            "--patch-operations",
            &patch_temp,
        ],
        3,
    );
    if !swap.status.success() {
        runner::error(&format!(
            "Truststore swap failed: {}",
            String::from_utf8_lossy(&swap.stderr).trim()
        ));
        return Ok(());
    }

    // Point domain back to canonical URI
    let restore = aws_run(
        &[
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            TRUSTSTORE_DOMAIN,
            "--patch-operations",
            &patch_canonical,
        ],
        3,
    );
    if !restore.status.success() {
        runner::error(&format!(
            "Truststore restore failed: {}",
            String::from_utf8_lossy(&restore.stderr).trim()
        ));
        return Ok(());
    }

    // Clean up temp key
    let _ = aws_run(&["aws", "s3", "rm", &temp_uri], 0);

    runner::success("mTLS truststore reloaded");
    Ok(())
}

fn aws_run(cmd: &[&str], retries: u32) -> std::process::Output {
    let mut result = Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .expect("failed to execute aws command");

    for _ in 0..retries {
        if result.status.success()
            || !String::from_utf8_lossy(&result.stderr).contains("TooManyRequests")
        {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
        result = Command::new(cmd[0])
            .args(&cmd[1..])
            .output()
            .expect("failed to execute aws command");
    }

    result
}
