"""End-to-end device onboarding command."""

from __future__ import annotations

from quickstart.commands.onboard_phases.phase_certs import run as certs
from quickstart.commands.onboard_phases.phase_flash import run as flash
from quickstart.commands.onboard_phases.phase_preflight import run as preflight
from quickstart.commands.onboard_phases.phase_register import run as register
from quickstart.commands.onboard_phases.phase_server import run as server
from quickstart.commands.onboard_phases.phase_verify import run as verify
from quickstart.commands.onboard_runner import run_phases
from quickstart.commands.onboard_types import OnboardContext
from quickstart.config import ProjectConfig

PHASES = [
    ("Preflight", preflight),
    ("Certificates", certs),
    ("Start Server", server),
    ("Register Device", register),
    ("Flash Firmware", flash),
    ("Verify Uplink", verify),
]


def run_onboard(
    config: ProjectConfig,
    *,
    device_name: str,
    owner_id: str,
    mode: str,
    verbose: bool,
    dry_run: bool,
    start_at: int = 0,
    skip: list[int] | None = None,
) -> int:
    ctx = OnboardContext(
        config=config,
        device_name=device_name,
        owner_id=owner_id,
        mode=mode,
        verbose=verbose,
        dry_run=dry_run,
    )
    return run_phases(PHASES, ctx, start_at=start_at, skip=skip)
