"""Parse bash-style .env files (KEY=VALUE with inline comments)."""

from __future__ import annotations

import os
from pathlib import Path


def load_env(env_file: Path) -> dict[str, str]:
    """Parse KEY=VALUE file, strip inline comments. Returns dict without mutating os.environ."""
    result: dict[str, str] = {}
    with open(env_file) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            key, sep, value = line.partition("=")
            if not sep:
                continue
            # Strip inline comments (space + #)
            if " #" in value:
                value = value[: value.index(" #")]
            value = value.strip().strip('"').strip("'")
            result[key.strip()] = value
    return result


def make_env(env_vars: dict[str, str]) -> dict[str, str]:
    """Merge env_vars into a copy of os.environ. Never mutates os.environ."""
    merged = os.environ.copy()
    merged.update(env_vars)
    return merged
