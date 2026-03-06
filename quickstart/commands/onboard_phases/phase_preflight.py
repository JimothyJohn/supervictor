"""Phase: pre-flight checks for required tools and services."""

from __future__ import annotations

from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
from quickstart.preflight import check_docker_running, check_tools

_BASE_TOOLS = ["openssl", "cargo", "espflash", "docker"]
_AWS_TOOLS = ["sam", "aws"]


def run(ctx: OnboardContext) -> PhaseResult:
    """Verify required CLI tools are present and Docker is running."""
    required = list(_BASE_TOOLS)
    if ctx.mode == "aws":
        required.extend(_AWS_TOOLS)

    missing = check_tools(required)
    if missing:
        return PhaseResult(
            PhaseStatus.FAILED,
            f"Missing tools: {', '.join(missing)}",
        )

    if not check_docker_running():
        return PhaseResult(PhaseStatus.FAILED, "Docker daemon is not running")

    if not ctx.config.env_dev.exists():
        return PhaseResult(
            PhaseStatus.FAILED,
            f".env.dev not found at {ctx.config.env_dev}",
        )

    return PhaseResult(PhaseStatus.PASSED)
