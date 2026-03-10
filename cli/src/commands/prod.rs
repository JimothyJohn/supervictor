use crate::config::ProjectConfig;
use crate::env;
use crate::error::CliError;
use crate::runner::{self, Runner};
use crate::sam::SamLocal;

use super::{dev, staging, truststore};

pub struct ProdArgs {
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn run_prod(args: &ProdArgs, config: &ProjectConfig, r: &dyn Runner) -> Result<i32, CliError> {
    // Gate 1: dev
    runner::milestone("Running dev gate");
    let dev_args = dev::DevArgs {
        verbose: args.verbose,
        dry_run: args.dry_run,
        serve: false,
        stop: false,
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
    truststore::reload(r, args.verbose, args.dry_run)?;

    if deployed {
        runner::success("\nProduction deployment complete.");
    } else {
        runner::success("\nNothing to deploy. Production stack is up to date.");
    }
    Ok(0)
}

#[cfg(test)]
#[path = "prod_tests.rs"]
mod tests;
