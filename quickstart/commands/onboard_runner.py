"""Phase runner for the onboard command."""

from collections.abc import Callable

from quickstart import output
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus

Phase = tuple[str, Callable[[OnboardContext], PhaseResult]]


def run_phases(
    phases: list[Phase],
    ctx: OnboardContext,
    *,
    start_at: int = 0,
    skip: list[int] | None = None,
) -> int:
    """Run a list of onboard phases sequentially.

    Returns exit code: 0 = all passed, 1 = failure, 130 = interrupted.
    """
    skip = skip or []

    try:
        for i, (name, fn) in enumerate(phases):
            if i < start_at:
                output.step(f"Skipping phase {i}: {name} (--start-at {start_at})")
                continue
            if i in skip:
                output.step(f"Skipping phase {i}: {name} (--skip)")
                continue

            output.milestone(f"Phase {i}: {name}")
            result = fn(ctx)

            if result.status == PhaseStatus.FAILED:
                output.error(f"Phase {i} failed: {result.message}")
                return 1
            elif result.status == PhaseStatus.SKIPPED:
                output.info(f"Phase {i} skipped: {result.message}")
            else:
                output.success(f"Phase {i}: {name}")

        output.success(f"Onboarding complete for {ctx.device_name}")
        return 0

    except KeyboardInterrupt:
        output.error(f"\nInterrupted at phase {i}. Resume with: --start-at {i}")
        return 130

    finally:
        if ctx.compose_file is not None:
            output.step("Stopping compose stack...")
            import subprocess

            subprocess.run(
                ["docker", "compose", "-f", str(ctx.compose_file), "down"],
                capture_output=True,
                timeout=30,
            )
        elif ctx.api_process is not None:
            output.step("Stopping API server...")
            ctx.api_process.terminate()
            ctx.api_process.wait(timeout=5)
