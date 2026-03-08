"""qs ping — mTLS GET request to verify the server is up and running."""

from __future__ import annotations

import argparse
import ssl
import sys
import urllib.request
from pathlib import Path

from quickstart import runner
from quickstart.config import ProjectConfig


def run_ping(args: argparse.Namespace, config: ProjectConfig) -> int:
    cert_dir = Path(args.certs) if args.certs else config.repo_root / "certs" / "devices" / "test-device"
    ca = Path(args.ca) if args.ca else config.repo_root / "certs" / "ca" / "ca.pem"

    client_cert = cert_dir / "client.pem"
    client_key = cert_dir / "client.key"

    for path, label in [(client_cert, "client cert"), (client_key, "client key"), (ca, "CA cert")]:
        if not path.exists():
            runner.error(f"{label} not found at {path}")
            return 1

    ctx = ssl.create_default_context(cafile=str(ca))
    ctx.load_cert_chain(certfile=str(client_cert), keyfile=str(client_key))

    url = f"https://{args.host}:{args.port}/"
    runner.step(f"Pinging {url}")

    if args.dry_run:
        print(f"  [dry-run] GET {url}")
        return 0

    try:
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, context=ctx, timeout=10) as resp:
            body = resp.read().decode()
            runner.success(f"Status: {resp.status}")
            print(body)
            return 0
    except urllib.error.HTTPError as e:
        # Server responded — it's up, even if the status isn't 2xx
        runner.success(f"Status: {e.code}")
        print(e.read().decode())
        return 0
    except Exception as e:
        runner.error(f"Error: {e}")
        return 1


def register_subparser(sub: argparse._SubParsersAction) -> None:
    p = sub.add_parser("ping", help="mTLS GET to verify the server is up")
    p.add_argument("--certs", default=None, help="Directory with client.pem and client.key (default: certs/devices/test-device)")
    p.add_argument("--ca", default=None, help="CA certificate (default: certs/ca/ca.pem)")
    p.add_argument("--host", default="localhost", help="Server host (default: localhost)")
    p.add_argument("--port", type=int, default=443, help="Server port (default: 443)")
