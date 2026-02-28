"""qs billing — Check billing status for an owner."""

from __future__ import annotations

import argparse
import json

import requests

from quickstart import runner
from quickstart.config import ProjectConfig
from quickstart.env import load_env


def run_billing(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Show owner billing status via admin API."""
    env_file = config.env_staging if getattr(args, "staging", False) else config.env_dev
    env_vars = load_env(env_file)
    host = env_vars.get("HOST", f"localhost:{config.sam_local_port}")
    api_path = env_vars.get("API_PATH", "")
    scheme = "https" if "localhost" not in host else "http"
    base_url = f"{scheme}://{host}{api_path}"

    runner.step(f"Fetching billing status for owner: {args.owner_id}")

    try:
        resp = requests.get(f"{base_url}/owners/{args.owner_id}", timeout=10)
    except requests.ConnectionError as exc:
        runner.error(f"Connection failed: {exc}")
        return 1

    if resp.status_code == 200:
        data = resp.json()
        print(json.dumps(data, indent=2))
        return 0
    elif resp.status_code == 404:
        runner.error(f"Owner '{args.owner_id}' not found")
        return 1
    else:
        runner.error(f"Failed ({resp.status_code}): {resp.text}")
        return 1
