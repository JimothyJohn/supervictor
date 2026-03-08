"""qs prod — Full pipeline: dev + staging gates, confirmation, prod deploy."""

from __future__ import annotations

import argparse
import logging

from quickstart import runner
from quickstart.commands import dev, staging
from quickstart.config import ProjectConfig
from quickstart.env import load_env, make_env
from quickstart.sam import SamLocal

logger = logging.getLogger(__name__)

_TRUSTSTORE_DOMAIN = "supervictor.advin.io"
_TRUSTSTORE_BUCKET = "supervictor"
_TRUSTSTORE_KEY = "truststore.pem"
_TRUSTSTORE_URI = f"s3://{_TRUSTSTORE_BUCKET}/{_TRUSTSTORE_KEY}"
_TRUSTSTORE_TEMP_KEY = "truststore-reload.pem"
_TRUSTSTORE_TEMP_URI = f"s3://{_TRUSTSTORE_BUCKET}/{_TRUSTSTORE_TEMP_KEY}"


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
    runner.step("Loading .env.prod")
    prod_vars = load_env(config.env_prod)
    env = make_env(prod_vars)
    sam = SamLocal(config, env=env, verbose=verbose, dry_run=dry_run)
    sam.build(no_cache=True)
    deployed = sam.deploy(config.sam_config_env_prod, force_upload=True)

    # Reload API Gateway mTLS truststore so it picks up any CA changes
    _reload_truststore(verbose=verbose, dry_run=dry_run)

    if deployed:
        runner.success("\nProduction deployment complete.")
    else:
        runner.success("\nNothing to deploy. Production stack is up to date.")
    return 0


def _reload_truststore(*, verbose: bool, dry_run: bool) -> None:
    """Force API Gateway to re-read the mTLS truststore from S3.

    API Gateway ignores update-domain-name when the URI hasn't changed,
    so we swap to a temp copy and back to force a real reload.
    """
    import subprocess
    import time

    runner.step("Reloading API Gateway mTLS truststore")
    if dry_run:
        logger.info("[dry-run] truststore reload skipped")
        return

    def _run(cmd: list[str], retries: int = 0, delay: float = 3.0) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(cmd, capture_output=True, text=True)
        for attempt in range(retries):
            if result.returncode == 0 or "TooManyRequests" not in result.stderr:
                break
            logger.info(f"Rate limited, retrying in {delay}s (attempt {attempt + 2}/{retries + 1})")
            time.sleep(delay)
            result = subprocess.run(cmd, capture_output=True, text=True)
        return result

    # Copy truststore to temp key
    cp = _run(["aws", "s3", "cp", _TRUSTSTORE_URI, _TRUSTSTORE_TEMP_URI])
    if cp.returncode != 0:
        runner.error(f"Truststore copy failed: {cp.stderr.strip()}")
        return

    # Point domain to temp URI
    swap = _run(
        [
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            _TRUSTSTORE_DOMAIN,
            "--patch-operations",
            f"op=replace,path=/mutualTlsAuthentication/truststoreUri,value={_TRUSTSTORE_TEMP_URI}",
        ],
        retries=3,
    )
    if swap.returncode != 0:
        runner.error(f"Truststore swap failed: {swap.stderr.strip()}")
        return

    # Point domain back to canonical URI
    restore = _run(
        [
            "aws",
            "apigateway",
            "update-domain-name",
            "--domain-name",
            _TRUSTSTORE_DOMAIN,
            "--patch-operations",
            f"op=replace,path=/mutualTlsAuthentication/truststoreUri,value={_TRUSTSTORE_URI}",
        ],
        retries=3,
    )
    if restore.returncode != 0:
        runner.error(f"Truststore restore failed: {restore.stderr.strip()}")
        return

    # Clean up temp key
    _run(["aws", "s3", "rm", _TRUSTSTORE_TEMP_URI])

    runner.success("mTLS truststore reloaded")
