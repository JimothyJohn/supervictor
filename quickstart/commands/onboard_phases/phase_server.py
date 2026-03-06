"""Phase: start the API server (on-prem Docker or AWS SAM local)."""

from __future__ import annotations

import time
import urllib.error
import urllib.request

from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
from quickstart.sam import SamLocal

_ONPREM_PORT = 8000
_POLL_INTERVAL = 1
_POLL_TIMEOUT = 30


def _wait_for_server(url: str, timeout: int = _POLL_TIMEOUT) -> bool:
    """Poll url until an HTTP response (any status) or timeout."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            urllib.request.urlopen(url, timeout=2)
            return True
        except urllib.error.HTTPError:
            return True  # Server is up, just returned an error code
        except (urllib.error.URLError, OSError):
            time.sleep(_POLL_INTERVAL)
    return False


def _start_onprem(ctx: OnboardContext) -> PhaseResult:
    """Build and start the Caddy + uplink compose stack for on-prem."""
    from quickstart.commands.onboard_phases.phase_server_compose import start_compose

    return start_compose(ctx)


def _write_sam_env_overrides(ctx: OnboardContext) -> str:
    """Write a JSON env-vars file to override Lambda env for SAM local."""
    import json

    env_file = ctx.config.log_dir / "sam_env_vars.json"
    env_file.parent.mkdir(parents=True, exist_ok=True)
    overrides = {
        "HelloWorldFunction": {
            "STORE_BACKEND": "sqlite",
            "SQLITE_DB_PATH": "/tmp/supervictor.db",
        }
    }
    env_file.write_text(json.dumps(overrides))
    return str(env_file)


def _start_aws(ctx: OnboardContext) -> PhaseResult:
    """Build and start SAM local with SQLite store (no DynamoDB locally)."""
    env_file = _write_sam_env_overrides(ctx)
    sam = SamLocal(
        ctx.config,
        verbose=ctx.verbose,
        dry_run=ctx.dry_run,
    )
    try:
        sam.build()
        sam.start(extra_args=["--env-vars", env_file])
        sam.wait_ready()
    except Exception as e:
        sam.stop()
        return PhaseResult(PhaseStatus.FAILED, f"SAM local failed: {e}")

    ctx.api_url = sam.url
    ctx.api_process = sam._proc
    return PhaseResult(PhaseStatus.PASSED)


def run(ctx: OnboardContext) -> PhaseResult:
    """Start the appropriate API server based on mode."""
    if ctx.mode == "onprem":
        return _start_onprem(ctx)
    return _start_aws(ctx)
