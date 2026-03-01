"""Output engine for the quickstart CLI.

Controls verbosity, emoji prefixes, and color theming. All output
flows through this module so a single global flag controls what
the user sees.

Verbosity levels:
  - default (verbose=False): milestones + errors + confirm only
  - verbose (verbose=True): everything (milestones + steps + $ commands)
"""

from __future__ import annotations

import sys

# ── ANSI color palette ──────────────────────────────────────────────
_RESET = "\033[0m"
_BOLD = "\033[1m"
_DIM = "\033[2m"
_RED = "\033[31m"
_GREEN = "\033[32m"
_CYAN = "\033[36m"
_BLUE = "\033[34m"
_MAGENTA = "\033[35m"
_BRIGHT_CYAN = "\033[96m"
_BRIGHT_YELLOW = "\033[93m"
_BRIGHT_MAGENTA = "\033[95m"

_ACCENT_CYCLE: list[str] = [
    _BRIGHT_CYAN,
    _MAGENTA,
    _BLUE,
    _BRIGHT_YELLOW,
    _BRIGHT_MAGENTA,
    _CYAN,
]

# ── Global state ────────────────────────────────────────────────────
_verbose: bool = False
_accent_index: int = 0


def set_verbose(verbose: bool) -> None:
    """Set global verbosity. Called once from __main__.py."""
    global _verbose
    _verbose = verbose


def is_verbose() -> bool:
    return _verbose


def _next_accent() -> str:
    global _accent_index
    color = _ACCENT_CYCLE[_accent_index % len(_ACCENT_CYCLE)]
    _accent_index += 1
    return color


# ── Public output functions ─────────────────────────────────────────

def milestone(msg: str, *, emoji: str = "\u2699\ufe0f ") -> None:
    """Major pipeline milestone. Always visible."""
    accent = _next_accent()
    print(f"\n{emoji}{_BOLD}{accent}{msg}{_RESET}")


def step(msg: str) -> None:
    """Intermediate step. Suppressed unless verbose."""
    if not _verbose:
        return
    print(f"\n{_BOLD}{_CYAN}=> {msg}{_RESET}")


def success(msg: str) -> None:
    """Success/completion. Always visible."""
    print(f"\u2705 {_BOLD}{_GREEN}{msg}{_RESET}")


def error(msg: str) -> None:
    """Error to stderr. Always visible."""
    print(f"\u274c {_BOLD}{_RED}{msg}{_RESET}", file=sys.stderr)


def detail(msg: str) -> None:
    """Detail line. Suppressed unless verbose."""
    if not _verbose:
        return
    print(f"  {_DIM}{msg}{_RESET}")


def info(msg: str) -> None:
    """Informational message. Always visible."""
    print(f"  {msg}")


def confirm(prompt: str) -> bool:
    """Interactive yes/no. Always visible."""
    try:
        answer = input(f"{_BOLD}{prompt}{_RESET}").strip().lower()
        return answer in ("y", "yes")
    except (EOFError, KeyboardInterrupt):
        print()
        return False
