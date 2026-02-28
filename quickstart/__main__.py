"""CLI entry point: python3 quickstart <command>"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Ensure the repo root is on sys.path so `quickstart` resolves as a package
_repo_root = str(Path(__file__).resolve().parent.parent)
if _repo_root not in sys.path:
    sys.path.insert(0, _repo_root)

from quickstart.commands import dev, edge, staging, prod  # noqa: E402
from quickstart.config import ProjectConfig  # noqa: E402


def _find_repo_root() -> Path:
    """Walk up from this file to find the repo root (contains .git/)."""
    p = Path(__file__).resolve().parent.parent
    while p != p.parent:
        if (p / ".git").exists():
            return p
        p = p.parent
    # Fallback: assume quickstart/ is directly inside repo root
    return Path(__file__).resolve().parent.parent


def main() -> int:
    parser = argparse.ArgumentParser(
        prog="quickstart",
        description="Supervictor quickstart CLI — dev, staging, and prod pipelines",
    )
    parser.add_argument("-v", "--verbose", action="store_true", help="Show full command output")
    parser.add_argument("--dry-run", action="store_true", help="Print commands without executing")

    sub = parser.add_subparsers(dest="command", required=True)

    dev_p = sub.add_parser("dev", help="Local dev cycle: unit tests + sam local + integration tests")
    dev_p.add_argument("--serve", action="store_true", help="Leave sam local running for manual testing")

    sub.add_parser("edge", help="Build, flash, and monitor the embedded device")

    sub.add_parser("staging", help="Dev gate + deploy to dev stack + remote tests")

    sub.add_parser("prod", help="Full pipeline + confirmation + prod deployment")

    args = parser.parse_args()
    config = ProjectConfig.from_repo_root(_find_repo_root())

    dispatch = {
        "dev": lambda: dev.run_dev(args, config),
        "edge": lambda: edge.run_edge(args, config),
        "staging": lambda: staging.run_staging(args, config),
        "prod": lambda: prod.run_prod(args, config),
    }

    return dispatch[args.command]()


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nInterrupted.")
        sys.exit(130)
