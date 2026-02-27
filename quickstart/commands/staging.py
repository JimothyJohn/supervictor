"""qs staging — Dev gate + deploy to dev stack + remote integration tests."""

from __future__ import annotations

import argparse

from quickstart import runner
from quickstart.commands import dev
from quickstart.config import ProjectConfig
from quickstart.env import load_env, make_env
from quickstart.preflight import require
from quickstart.sam import SamLocal


def _ensure_certs(config: ProjectConfig, env: dict[str, str], verbose: bool, dry_run: bool) -> None:
    """Generate test CA + device cert if missing."""
    certs_dir = config.cloud_dir / config.certs_dir_name
    gen_script = config.cloud_dir / config.gen_certs_script

    if not (certs_dir / "ca" / "ca.pem").exists():
        runner.step("Generating test CA")
        runner.run(
            [str(gen_script), "ca"],
            cwd=config.cloud_dir, env=env, verbose=verbose, dry_run=dry_run,
        )

    if not (certs_dir / "devices" / "test-device" / "client.pem").exists():
        runner.step("Generating test-device certificate")
        runner.run(
            [str(gen_script), "device", "test-device"],
            cwd=config.cloud_dir, env=env, verbose=verbose, dry_run=dry_run,
        )


def run_staging(
    args: argparse.Namespace,
    config: ProjectConfig,
    *,
    skip_dev_gate: bool = False,
) -> int:
    """Execute the staging pipeline. Returns 0 on success."""
    verbose = getattr(args, "verbose", False)
    dry_run = getattr(args, "dry_run", False)

    # Gate: run full dev pipeline first (skip if caller already ran it)
    if not skip_dev_gate:
        runner.step("Running dev gate")
        dev_args = argparse.Namespace(verbose=verbose, dry_run=dry_run, serve=False)
        rc = dev.run_dev(dev_args, config)
        if rc != 0:
            runner.error("Dev pipeline failed. Aborting staging.")
            return rc

    # Load staging env
    runner.step("Loading .env.staging")
    staging_vars = load_env(config.env_staging)
    env = make_env(staging_vars)

    # Preflight
    require(["uv", "sam", "docker"], need_docker=True)

    # Deploy to dev stack
    sam = SamLocal(config, env=env, verbose=verbose, dry_run=dry_run)
    sam.build()
    sam.deploy(config.sam_config_env_dev)

    # Run integration tests against the deployed dev stack.
    # The dev stack has no mTLS/custom domain (those are prod-only), so we reuse
    # the "local" marker tests against the execute-api HTTPS endpoint.
    # Tests do f"{sam_local_url}/hello", so the base URL must include the stage
    # prefix (e.g. https://host/dev) derived from API_PATH (e.g. /dev/hello).
    runner.step("Running integration tests against deployed dev stack")
    host = staging_vars.get("HOST", "")
    api_path = staging_vars.get("API_PATH", "")
    # Strip /hello from API_PATH to get the stage prefix (e.g. /dev/hello → /dev)
    stage_prefix = api_path.rsplit("/hello", 1)[0]
    sam_local_url = f"https://{host}{stage_prefix}"

    test_env = make_env({**staging_vars, "SAM_LOCAL_URL": sam_local_url})

    try:
        runner.run(
            ["uv", "run", "pytest", "tests/integration/", "-m", "local", "-v"],
            cwd=config.cloud_dir, env=test_env, verbose=verbose, dry_run=dry_run,
        )
    except Exception:
        runner.error("Staging integration tests failed.")
        return 1

    runner.success("\nStaging pipeline passed.")
    return 0
