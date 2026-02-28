"""qs register — Register devices and owners via the admin API."""

from __future__ import annotations

import argparse
import json

import requests

from quickstart import runner
from quickstart.config import ProjectConfig
from quickstart.env import load_env


def run_register(args: argparse.Namespace, config: ProjectConfig) -> int:
    """Dispatch to register-device or register-owner."""
    verbose = getattr(args, "verbose", False)
    target = getattr(args, "target", "")

    env_file = config.env_staging if getattr(args, "staging", False) else config.env_dev
    env_vars = load_env(env_file)
    host = env_vars.get("HOST", f"localhost:{config.sam_local_port}")
    api_path = env_vars.get("API_PATH", "")
    scheme = "https" if "localhost" not in host else "http"
    base_url = f"{scheme}://{host}{api_path}"

    if target == "device":
        return _register_device(args, base_url, verbose)
    elif target == "owner":
        return _register_owner(args, base_url, verbose)
    else:
        runner.error(f"Unknown register target: {target}")
        return 1


def _register_device(args: argparse.Namespace, base_url: str, verbose: bool) -> int:
    runner.step(f"Registering device: {args.device_id}")
    payload: dict[str, str] = {
        "device_id": args.device_id,
        "owner_id": args.owner_id,
    }
    if getattr(args, "subject_dn", None):
        payload["subject_dn"] = args.subject_dn

    try:
        resp = requests.post(f"{base_url}/devices", json=payload, timeout=10)
    except requests.ConnectionError as exc:
        runner.error(f"Connection failed: {exc}")
        return 1

    if resp.status_code in (200, 201):
        runner.success(f"Device registered: {json.dumps(resp.json(), indent=2)}")
        return 0
    else:
        runner.error(f"Failed ({resp.status_code}): {resp.text}")
        return 1


def _register_owner(args: argparse.Namespace, base_url: str, verbose: bool) -> int:
    runner.step(f"Registering owner: {args.owner_id}")
    payload = {
        "owner_id": args.owner_id,
        "email": args.email,
    }

    try:
        resp = requests.post(f"{base_url}/owners", json=payload, timeout=10)
    except requests.ConnectionError as exc:
        runner.error(f"Connection failed: {exc}")
        return 1

    if resp.status_code in (200, 201):
        runner.success(f"Owner registered: {json.dumps(resp.json(), indent=2)}")
        return 0
    else:
        runner.error(f"Failed ({resp.status_code}): {resp.text}")
        return 1
