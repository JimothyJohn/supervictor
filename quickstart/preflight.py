"""Pre-flight checks for required tools and services."""

from __future__ import annotations

import shutil
import subprocess
import sys

from quickstart import runner


def check_tools(required: list[str]) -> list[str]:
    """Return list of tools not found on PATH."""
    return [t for t in required if shutil.which(t) is None]


def check_docker_running() -> bool:
    """Return True if Docker daemon is responsive."""
    try:
        subprocess.run(
            ["docker", "info"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=True,
        )
        return True
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False


def require(tools: list[str], need_docker: bool = False) -> None:
    """Exit with actionable message if any tool is missing or Docker is down."""
    missing = check_tools(tools)
    if missing:
        runner.error(f"Missing required tools: {', '.join(missing)}")
        runner.error("Install them and try again.")
        sys.exit(1)

    if need_docker and not check_docker_running():
        runner.error("Docker daemon is not running. Start Docker and try again.")
        sys.exit(1)
