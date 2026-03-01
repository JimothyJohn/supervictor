"""qs prod — Full pipeline: dev + staging gates, confirmation, prod deploy."""

from __future__ import annotations

import argparse

from quickstart import runner
from quickstart.commands import dev, staging
from quickstart.config import ProjectConfig
from quickstart.env import make_env
from quickstart.sam import SamLocal


def run_prod(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Execute the prod pipeline. Returns 0 on success."""
    verbose = getattr(args, "verbose", False)
    dry_run = getattr(args, "dry_run", False)

    # Gate 1: dev
    runner.milestone("Running dev gate", emoji="\U0001f6aa ")
    dev_args = argparse.Namespace(verbose=verbose, dry_run=dry_run, serve=False)
    rc = dev.run_dev(dev_args, config)
    if rc != 0:
        runner.error("Dev pipeline failed. Aborting prod deployment.")
        return rc

    # Gate 2: staging (skip dev gate since we already ran it)
    runner.milestone("Running staging gate", emoji="\U0001f6aa ")
    staging_args = argparse.Namespace(verbose=verbose, dry_run=dry_run)
    rc = staging.run_staging(staging_args, config, skip_dev_gate=True)
    if rc != 0:
        runner.error("Staging pipeline failed. Aborting prod deployment.")
        return rc

    # Confirmation
    print()
    if not runner.confirm("All tests passed. Deploy to PRODUCTION? [y/N] "):
        print("Aborted.")
        return 1

    # Deploy to prod
    env = make_env({})
    sam = SamLocal(config, env=env, verbose=verbose, dry_run=dry_run)
    sam.build(no_cache=True)
    deployed = sam.deploy(config.sam_config_env_prod, force_upload=True)

    if deployed:
        runner.success("\nProduction deployment complete.")
    else:
        runner.success("\nNothing to deploy. Production stack is up to date.")
    return 0
