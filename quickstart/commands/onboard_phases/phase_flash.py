"""Phase: build and flash the device firmware."""

from __future__ import annotations

import subprocess

from quickstart import runner
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
from quickstart.env import load_env, make_env


def run(ctx: OnboardContext) -> PhaseResult:
    """Build and flash firmware with DEVICE_NAME set in environment.

    Uses cargo build + espflash flash (no --monitor) so the process exits
    cleanly without needing an interactive terminal.
    """
    env_vars = load_env(ctx.config.env_dev)
    env_vars["DEVICE_NAME"] = ctx.device_name
    env = make_env(env_vars)

    try:
        runner.run(
            ["cargo", "build", "--bin", "supervictor-embedded", "--features", "embedded"],
            cwd=ctx.config.device_dir,
            env=env,
            verbose=ctx.verbose,
            dry_run=ctx.dry_run,
        )
        runner.run(
            [
                "espflash",
                "flash",
                "--chip",
                "esp32c3",
                "target/riscv32imc-unknown-none-elf/debug/supervictor-embedded",
            ],
            cwd=ctx.config.device_dir,
            env=env,
            verbose=ctx.verbose,
            dry_run=ctx.dry_run,
        )
    except subprocess.CalledProcessError as e:
        return PhaseResult(PhaseStatus.FAILED, f"Flash failed: {e}")

    return PhaseResult(PhaseStatus.PASSED)
