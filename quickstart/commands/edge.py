"""qs edge — Build, flash, and monitor the embedded device."""

from __future__ import annotations

import argparse

from quickstart import runner
from quickstart.config import ProjectConfig
from quickstart.env import load_env, make_env
from quickstart.preflight import require


def run_edge(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Flash the embedded firmware and stream serial output. Returns 0 on success."""
    verbose = getattr(args, "verbose", False)
    dry_run = getattr(args, "dry_run", False)

    require(["cargo", "espflash"])

    runner.step("Loading .env.dev")
    env_vars = load_env(config.env_dev)
    env = make_env(env_vars)

    runner.step("Building and flashing embedded firmware")
    port = env_vars.get("ESPFLASH_PORT", "")
    if port:
        runner.step(f"Using serial port {port}")
    try:
        cmd = ["cargo", "run", "--bin", "supervictor-embedded", "--features", "embedded"]
        if port:
            cmd.extend(["--", "--port", port])
        runner.run(
            cmd,
            cwd=config.device_dir,
            env=env,
            verbose=verbose,
            dry_run=dry_run,
        )
    except Exception:
        runner.error("Flash failed.")
        return 1

    return 0
