"""Subprocess wrapper with logging, dry-run support, and interactive prompts."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from quickstart.output import is_verbose

# ANSI colors
_BOLD = "\033[1m"
_CYAN = "\033[36m"
_GREEN = "\033[32m"
_RED = "\033[31m"
_RESET = "\033[0m"


def step(msg: str) -> None:
    """Print a step header. Suppressed unless verbose."""
    if not is_verbose():
        return
    print(f"\n{_BOLD}{_CYAN}=> {msg}{_RESET}")


def milestone(msg: str, *, emoji: str = "") -> None:
    """Print a major milestone header with optional emoji."""
    print(f"\n{_BOLD}{_GREEN}{emoji}== {msg} =={_RESET}")


def success(msg: str) -> None:
    """Print a success message."""
    print(f"{_GREEN}{msg}{_RESET}")


def error(msg: str) -> None:
    """Print an error message to stderr."""
    print(f"{_RED}{msg}{_RESET}", file=sys.stderr)


def run(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
    capture: bool = False,
    verbose: bool = False,
    dry_run: bool = False,
    log_to: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a command synchronously. Raises CalledProcessError on non-zero if check=True."""
    cmd_str = " ".join(cmd)
    if dry_run:
        print(f"  [dry-run] {cmd_str}")
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

    if verbose:
        print(f"  $ {cmd_str}")

    kwargs: dict = dict(cwd=cwd, env=env, check=check)
    if log_to:
        log_to.parent.mkdir(parents=True, exist_ok=True)
        kwargs["stdout"] = subprocess.PIPE
        kwargs["stderr"] = subprocess.STDOUT
        kwargs["text"] = True
        kwargs["check"] = False
        result = subprocess.run(cmd, **kwargs)
        log_to.write_text(result.stdout or "")
        if verbose:
            print(result.stdout, end="")
        if check and result.returncode != 0:
            raise subprocess.CalledProcessError(
                result.returncode, cmd, result.stdout, result.stderr
            )
        return result

    if capture:
        kwargs["capture_output"] = True
        kwargs["text"] = True

    return subprocess.run(cmd, **kwargs)


def start_background(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    log_file: str | None = None,
    verbose: bool = False,
    dry_run: bool = False,
) -> subprocess.Popen | None:
    """Start a command in the background. Returns Popen handle."""
    cmd_str = " ".join(cmd)
    if dry_run:
        print(f"  [dry-run] {cmd_str} &")
        return None

    if verbose:
        print(f"  $ {cmd_str} &")

    stdout = open(log_file, "w") if log_file else subprocess.DEVNULL
    stderr = subprocess.STDOUT if log_file else subprocess.DEVNULL

    return subprocess.Popen(cmd, cwd=cwd, env=env, stdout=stdout, stderr=stderr)


def confirm(prompt: str) -> bool:
    """Interactive yes/no confirmation. Returns True for 'y' or 'yes'."""
    try:
        answer = input(f"{_BOLD}{prompt}{_RESET}").strip().lower()
        return answer in ("y", "yes")
    except (EOFError, KeyboardInterrupt):
        print()
        return False
