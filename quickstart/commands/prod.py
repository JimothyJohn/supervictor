"""qs prod — Full pipeline: dev + staging gates, confirmation, prod deploy."""

from __future__ import annotations

import argparse

from quickstart import runner
from quickstart.commands import dev, staging
from quickstart.commands.staging import _ensure_certs
from quickstart.config import ProjectConfig
from quickstart.env import load_env, make_env
from quickstart.preflight import require
from quickstart.sam import SamLocal


def run_prod(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Execute the prod pipeline. Returns 0 on success."""
    verbose = getattr(args, "verbose", False)
    dry_run = getattr(args, "dry_run", False)

    # Gate 1: dev
    runner.step("Running dev gate")
    dev_args = argparse.Namespace(verbose=verbose, dry_run=dry_run, serve=False)
    rc = dev.run_dev(dev_args, config)
    if rc != 0:
        runner.error("Dev pipeline failed. Aborting prod deployment.")
        return rc

    # Gate 2: staging (skip dev gate since we already ran it)
    runner.step("Running staging gate")
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
    sam.build()
    sam.deploy(config.sam_config_env_prod)

    # Ensure certs exist for mTLS verification tests
    require(["openssl"])
    _ensure_certs(config, env, verbose, dry_run)

    # Verify: run remote mTLS tests against prod custom domain
    runner.step("Running mTLS verification tests against production")
    certs_dir = str(config.repo_root / config.certs_dir_name)
    test_env = make_env({
        "API_ENDPOINT": config.prod_api_endpoint,
        "TEST_CERT_DIR": certs_dir,
    })

    try:
        runner.run(
            ["uv", "run", "pytest", "tests/integration/", "-m", "remote", "-v"],
            cwd=config.cloud_dir, env=test_env, verbose=verbose, dry_run=dry_run,
        )
    except Exception:
        runner.error("Production verification tests failed!")
        return 1

    runner.success("\nProduction deployment complete.")
    return 0
