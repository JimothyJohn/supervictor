"""Phase helper: start on-prem server via Docker Compose (Caddy + uplink)."""

from __future__ import annotations

import logging
import socket
import ssl
import time
import urllib.error
import urllib.request
from pathlib import Path

from quickstart import runner
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus

logger = logging.getLogger(__name__)

_POLL_INTERVAL = 2
_POLL_TIMEOUT = 60


def detect_lan_ip() -> str:
    """Return the host's LAN IP via a UDP socket trick (no traffic sent)."""
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.connect(("10.255.255.255", 1))
        return s.getsockname()[0]


def _ensure_server_cert(ctx: OnboardContext, host_ip: str) -> Path:
    """Generate a server cert for Caddy if one doesn't exist. Returns cert dir."""
    certs_dir = ctx.config.repo_root / "certs"
    server_dir = certs_dir / "servers" / "caddy"
    if (server_dir / "server.pem").exists() and (server_dir / "server.key").exists():
        logger.info("Server cert already exists at %s", server_dir)
        return server_dir

    gen_certs = ctx.config.cloud_dir / ctx.config.gen_certs_script
    logger.info("Generating server cert for host IP %s", host_ip)
    runner.run(
        [str(gen_certs), "server", "caddy", host_ip],
        verbose=ctx.verbose,
        dry_run=ctx.dry_run,
    )
    return server_dir


def _wait_for_https(
    url: str,
    ca_pem: Path,
    client_cert: Path,
    client_key: Path,
    timeout: int = _POLL_TIMEOUT,
) -> bool:
    """Poll HTTPS endpoint with mTLS until reachable or timeout."""
    ssl_ctx = ssl.create_default_context(cafile=str(ca_pem))
    ssl_ctx.load_cert_chain(certfile=str(client_cert), keyfile=str(client_key))

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            urllib.request.urlopen(url, timeout=3, context=ssl_ctx)
            return True
        except urllib.error.HTTPError:
            return True  # Server responded, just an error code
        except (urllib.error.URLError, OSError):
            time.sleep(_POLL_INTERVAL)
    return False


def start_compose(ctx: OnboardContext) -> PhaseResult:
    """Build and start Docker Compose stack (Caddy + uplink)."""
    cloud_dir = ctx.config.cloud_dir
    compose_file = cloud_dir / "docker-compose.yml"

    if not compose_file.exists():
        return PhaseResult(PhaseStatus.FAILED, f"Missing {compose_file}")

    try:
        host_ip = detect_lan_ip()
    except OSError as e:
        return PhaseResult(PhaseStatus.FAILED, f"Cannot detect LAN IP: {e}")

    logger.info("Detected LAN IP: %s", host_ip)

    # Ensure server cert exists for Caddy
    try:
        _ensure_server_cert(ctx, host_ip)
    except Exception as e:
        return PhaseResult(PhaseStatus.FAILED, f"Server cert generation failed: {e}")

    # Tear down any pre-existing stack to avoid port conflicts / stale containers
    ctx.config.log_dir.mkdir(parents=True, exist_ok=True)

    try:
        runner.run(
            ["docker", "compose", "-f", str(compose_file), "down", "--remove-orphans"],
            verbose=ctx.verbose,
            dry_run=ctx.dry_run,
            log_to=ctx.config.log_dir / "compose_down.log",
        )
    except Exception as e:
        logger.warning("Pre-cleanup compose down failed (non-fatal): %s", e)

    # Start compose stack
    try:
        runner.run(
            ["docker", "compose", "-f", str(compose_file), "up", "-d", "--build"],
            verbose=ctx.verbose,
            dry_run=ctx.dry_run,
            log_to=ctx.config.log_dir / "compose_up.log",
        )
    except Exception as e:
        return PhaseResult(PhaseStatus.FAILED, f"Compose up failed: {e}")

    ctx.compose_file = compose_file
    ctx.api_url = f"https://{host_ip}"

    if ctx.dry_run:
        return PhaseResult(PhaseStatus.PASSED)

    # Wait for HTTPS readiness using a device client cert for the mTLS check
    certs_dir = ctx.config.repo_root / "certs"
    ca_pem = certs_dir / "ca" / "ca.pem"
    device_cert = certs_dir / "devices" / ctx.device_name / "client.pem"
    device_key = certs_dir / "devices" / ctx.device_name / "client.key"

    if not _wait_for_https(ctx.api_url, ca_pem, device_cert, device_key):
        return PhaseResult(PhaseStatus.FAILED, "Compose stack did not become ready in time")

    logger.info("Compose stack ready at %s", ctx.api_url)
    return PhaseResult(PhaseStatus.PASSED)


def stop_compose(ctx: OnboardContext) -> None:
    """Tear down the Docker Compose stack."""
    if ctx.compose_file is None:
        return
    runner.run(
        ["docker", "compose", "-f", str(ctx.compose_file), "down"],
        verbose=ctx.verbose,
        dry_run=ctx.dry_run,
    )
