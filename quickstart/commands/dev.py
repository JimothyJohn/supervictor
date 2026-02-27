"""qs dev — Local development cycle: unit tests, sam local, integration tests."""

from __future__ import annotations

import argparse

from quickstart import runner
from quickstart.config import ProjectConfig
from quickstart.env import load_env, make_env
from quickstart.preflight import require
from quickstart.sam import SamLocal


def run_dev(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Execute the dev pipeline. Returns 0 on success, non-zero on failure."""
    verbose = getattr(args, "verbose", False)
    dry_run = getattr(args, "dry_run", False)
    serve = getattr(args, "serve", False)

    # Load env (returns dict, no os.environ mutation)
    runner.step("Loading .env.dev")
    env_vars = load_env(config.env_dev)
    env = make_env(env_vars)

    port = env_vars.get("SAM_LOCAL_PORT", str(config.sam_local_port))
    cfg = config if str(config.sam_local_port) == port else ProjectConfig(
        repo_root=config.repo_root,
        cloud_dir=config.cloud_dir,
        env_dev=config.env_dev,
        env_staging=config.env_staging,
        sam_local_port=int(port),
    )

    # Preflight
    require(["uv", "sam", "docker"], need_docker=True)

    # Unit tests
    runner.step("Running unit tests")
    try:
        runner.run(
            ["uv", "run", "pytest", "tests/unit/", "-v"],
            cwd=cfg.cloud_dir, env=env, verbose=verbose, dry_run=dry_run,
        )
    except Exception:
        runner.error("Unit tests failed.")
        return 1

    # SAM build
    sam = SamLocal(cfg, env=env, verbose=verbose, dry_run=dry_run)
    sam.build()

    # Start sam local + run integration tests (or serve)
    try:
        with sam:
            if serve:
                print(f"\n  sam local running at {sam.url}")
                print(f"  GET  {sam.url}/hello")
                print(f"  POST {sam.url}/hello  -d '{{\"id\":\"test\",\"current\":42}}'")
                print("\n  Press Ctrl+C to stop.")
                try:
                    sam._proc.wait()  # block until killed
                except KeyboardInterrupt:
                    pass
            else:
                runner.step("Running local integration tests")
                test_env = make_env({**env_vars, "SAM_LOCAL_URL": sam.url})
                runner.run(
                    ["uv", "run", "pytest", "tests/integration/", "-m", "local", "-v"],
                    cwd=cfg.cloud_dir, env=test_env, verbose=verbose, dry_run=dry_run,
                )
    except TimeoutError as e:
        runner.error(str(e))
        return 1
    except Exception:
        runner.error("Integration tests failed.")
        return 1

    runner.success("\nDev pipeline passed.")
    return 0
