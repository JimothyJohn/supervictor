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
    certs_dir = config.repo_root / config.certs_dir_name
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
    require(["uv", "sam", "docker", "openssl"], need_docker=True)

    # Deploy to dev stack
    sam = SamLocal(config, env=env, verbose=verbose, dry_run=dry_run)
    sam.build()
    sam.deploy(config.sam_config_env_dev)

    # Run integration tests against the deployed dev stack.
    # The dev stack has no mTLS/custom domain (those are prod-only), so we reuse
    # the "local" marker tests against the execute-api HTTPS endpoint.
    # execute-api URL format: https://{api-id}.execute-api.{region}.amazonaws.com/{stage}
    runner.step("Running integration tests against deployed dev stack")
    host = staging_vars.get("HOST", "")
    sam_local_url = f"https://{host}/{config.sam_config_env_dev}"

    test_env = make_env({**staging_vars, "SAM_LOCAL_URL": sam_local_url})

    log_dir = config.log_dir
    try:
        runner.run(
            ["uv", "run", "pytest", "tests/integration/", "-m", "local", "-v"],
            cwd=config.cloud_dir, env=test_env, verbose=verbose, dry_run=dry_run,
            log_to=log_dir / "staging_integration_tests.log",
        )
        runner.success("Staging integration tests passed")
    except Exception:
        runner.error(f"Staging integration tests failed (see {log_dir / 'staging_integration_tests.log'})")
        return 1

    # Run Rust device integration tests against the deployed dev stack.
    # Uses reqwest (HTTPS) from the desktop feature to validate that the
    # device's JSON payloads are accepted by the live Lambda.
    runner.step("Running Rust device integration tests against deployed stack")
    from quickstart.rust import host_target

    try:
        rust_target = host_target()
        device_test_env = make_env({**staging_vars, "DEPLOYED_URL": sam_local_url})
        runner.run(
            [
                "cargo", "test",
                "--test", "deployed_roundtrip",
                "--target", rust_target,
            ],
            cwd=config.device_dir,
            env=device_test_env,
            verbose=verbose,
            dry_run=dry_run,
            log_to=log_dir / "device_deployed_tests.log",
        )
        runner.success("Rust device integration tests passed")
    except Exception:
        runner.error(
            f"Rust device integration tests failed "
            f"(see {log_dir / 'device_deployed_tests.log'})"
        )
        return 1

    # Verify mTLS against prod custom domain (certs, not code).
    # The dev stack's execute-api URL has no mTLS — only the prod custom domain
    # (supervictor.advin.io) enforces client certificates via S3 truststore.
    runner.step("Verifying mTLS against production endpoint")
    _ensure_certs(config, env, verbose, dry_run)

    certs_dir = str(config.repo_root / config.certs_dir_name)
    mtls_env = make_env({
        **staging_vars,
        "API_ENDPOINT": config.prod_api_endpoint,
        "TEST_CERT_DIR": certs_dir,
    })

    try:
        runner.run(
            ["uv", "run", "pytest", "tests/integration/", "-m", "remote", "-v"],
            cwd=config.cloud_dir, env=mtls_env, verbose=verbose, dry_run=dry_run,
            log_to=log_dir / "mtls_tests.log",
        )
        runner.success("mTLS verification passed")
    except Exception:
        runner.error(f"mTLS verification failed (see {log_dir / 'mtls_tests.log'})")
        return 1

    runner.success("\nStaging pipeline passed.")
    return 0
