"""Phase: verify device is sending uplinks to the API."""

from __future__ import annotations

import json
import ssl
import time
import urllib.error
import urllib.request

from quickstart import output
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
from quickstart.env import load_env

_POLL_INTERVAL = 5
_POLL_TIMEOUT = 60


def _build_ssl_context(ctx: OnboardContext, *, use_local_ca: bool = False) -> ssl.SSLContext | None:
    """Create an mTLS SSL context using the device's client certificate."""
    if ctx.certs_dir is None:
        return None
    client_pem = ctx.certs_dir / "devices" / ctx.device_name / "client.pem"
    client_key = ctx.certs_dir / "devices" / ctx.device_name / "client.key"
    if not client_pem.exists() or not client_key.exists():
        return None
    if use_local_ca:
        ca_pem = ctx.certs_dir / "ca" / "ca.pem"
        ssl_ctx = ssl.create_default_context(cafile=str(ca_pem))
    else:
        ssl_ctx = ssl.create_default_context()
    ssl_ctx.load_cert_chain(certfile=str(client_pem), keyfile=str(client_key))
    return ssl_ctx


def _resolve_verify_url(ctx: OnboardContext) -> tuple[str, ssl.SSLContext | None]:
    """Determine the URL and SSL context for verification polling."""
    try:
        env_vars = load_env(ctx.config.env_dev)
    except FileNotFoundError:
        return f"{ctx.api_url}/devices/{ctx.device_name}/uplinks", None

    host = env_vars.get("HOST", "")
    port = env_vars.get("PORT", "")

    if ctx.mode == "onprem":
        if ctx.compose_file is not None:
            # Compose stack — go through Caddy HTTPS with mTLS + local CA
            ssl_ctx = _build_ssl_context(ctx, use_local_ca=True)
            return f"{ctx.api_url}/devices/{ctx.device_name}/uplinks", ssl_ctx
        # Legacy plain HTTP path
        base = f"http://{host}:{port}" if host and port else ctx.api_url
        return f"{base}/devices/{ctx.device_name}/uplinks", None

    if host.startswith("localhost") or host.startswith("127.0.0.1"):
        return f"{ctx.api_url}/devices/{ctx.device_name}/uplinks", None

    # Device targets a remote host — verify via mTLS
    ssl_ctx = _build_ssl_context(ctx)
    url = f"https://{host}/devices/{ctx.device_name}/uplinks"
    output.info(f"Device targets {host} — verifying via mTLS")
    return url, ssl_ctx


def run(ctx: OnboardContext) -> PhaseResult:
    """Poll uplinks endpoint until at least one record appears."""
    if ctx.dry_run:
        return PhaseResult(PhaseStatus.PASSED, "dry-run")

    url, ssl_ctx = _resolve_verify_url(ctx)
    output.info(f"Polling {url} (timeout {_POLL_TIMEOUT}s)")
    deadline = time.monotonic() + _POLL_TIMEOUT
    elapsed = 0

    while time.monotonic() < deadline:
        try:
            resp = urllib.request.urlopen(url, timeout=5, context=ssl_ctx)
            body = json.loads(resp.read().decode())
            if isinstance(body, list) and len(body) >= 1:
                return PhaseResult(PhaseStatus.PASSED)
            output.info(f"  No uplinks yet ({elapsed}s / {_POLL_TIMEOUT}s)")
        except (urllib.error.HTTPError, urllib.error.URLError, OSError) as exc:
            output.info(f"  Waiting for server ({elapsed}s / {_POLL_TIMEOUT}s): {exc}")
        time.sleep(_POLL_INTERVAL)
        elapsed += _POLL_INTERVAL

    return PhaseResult(PhaseStatus.FAILED, f"No uplinks received within {_POLL_TIMEOUT}s")
