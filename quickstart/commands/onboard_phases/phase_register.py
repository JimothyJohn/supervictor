"""Phase: register the device with the API server."""

from __future__ import annotations

import json
import ssl
import urllib.error
import urllib.request

from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus


def _build_ssl_context(ctx: OnboardContext) -> ssl.SSLContext | None:
    """Build an SSL context for HTTPS requests when using on-prem compose stack."""
    if ctx.compose_file is None or ctx.certs_dir is None:
        return None
    ca_pem = ctx.certs_dir / "ca" / "ca.pem"
    device_cert = ctx.certs_dir / "devices" / ctx.device_name / "client.pem"
    device_key = ctx.certs_dir / "devices" / ctx.device_name / "client.key"
    ssl_ctx = ssl.create_default_context(cafile=str(ca_pem))
    ssl_ctx.load_cert_chain(certfile=str(device_cert), keyfile=str(device_key))
    return ssl_ctx


def run(ctx: OnboardContext) -> PhaseResult:
    """POST device registration, then GET to verify status=active."""
    if ctx.dry_run:
        return PhaseResult(PhaseStatus.PASSED, "dry-run")

    ssl_ctx = _build_ssl_context(ctx)
    register_url = f"{ctx.api_url}/devices"
    payload = json.dumps(
        {
            "device_id": ctx.device_name,
            "owner_id": ctx.owner_id,
            "subject_dn": ctx.subject_dn,
        }
    ).encode()

    req = urllib.request.Request(
        register_url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    try:
        resp = urllib.request.urlopen(req, context=ssl_ctx)
        if resp.status != 201:
            return PhaseResult(
                PhaseStatus.FAILED,
                f"Registration returned HTTP {resp.status}, expected 201",
            )
    except urllib.error.HTTPError as e:
        return PhaseResult(
            PhaseStatus.FAILED,
            f"Registration failed: HTTP {e.code} — {e.read().decode(errors='replace')}",
        )
    except urllib.error.URLError as e:
        return PhaseResult(PhaseStatus.FAILED, f"Cannot reach API: {e.reason}")

    # Verify device is active
    verify_url = f"{ctx.api_url}/devices/{ctx.device_name}"
    try:
        resp = urllib.request.urlopen(verify_url, context=ssl_ctx)
        body = json.loads(resp.read().decode())
        if body.get("status") != "active":
            return PhaseResult(
                PhaseStatus.FAILED,
                f"Device status is '{body.get('status')}', expected 'active'",
            )
    except (urllib.error.HTTPError, urllib.error.URLError) as e:
        return PhaseResult(PhaseStatus.FAILED, f"Verification GET failed: {e}")

    return PhaseResult(PhaseStatus.PASSED)
