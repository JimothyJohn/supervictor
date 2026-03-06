"""Tests verifying CA_PATH propagation through the flash phase.

The device firmware reads CA_PATH at compile time via env!("CA_PATH").
The flash phase must pass this through to cargo build via the environment.
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from quickstart.commands.onboard_types import OnboardContext, PhaseStatus
from quickstart.config import ProjectConfig


def _make_ctx(tmp_path: Path, *, mode: str = "onprem") -> OnboardContext:
    config = ProjectConfig.from_repo_root(tmp_path)
    return OnboardContext(
        config=config,
        device_name="test-device",
        owner_id="owner-123",
        mode=mode,
        verbose=False,
        dry_run=False,
    )


class TestFlashCaPath:
    def test_ca_path_from_env_dev_passed_to_cargo(self, tmp_path: Path):
        """CA_PATH from .env.dev should be in the cargo build environment."""
        ctx = _make_ctx(tmp_path)

        captured_envs: list[dict[str, str]] = []

        def capture_run(cmd, *, cwd=None, env=None, **kwargs):
            if env:
                captured_envs.append(env)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=capture_run,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={
                    "HOST": "10.0.0.42",
                    "CA_PATH": "../../certs/ca/ca.pem",
                },
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env",
                side_effect=lambda v: v,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        # Both cargo build and espflash should receive CA_PATH
        assert len(captured_envs) == 2
        for env in captured_envs:
            assert "CA_PATH" in env
            assert env["CA_PATH"] == "../../certs/ca/ca.pem"

    def test_ca_path_onprem_uses_local_ca(self, tmp_path: Path):
        """On-prem mode should use local CA path (certs/ca/ca.pem)."""
        ctx = _make_ctx(tmp_path, mode="onprem")

        captured_envs: list[dict[str, str]] = []

        def capture_run(cmd, *, cwd=None, env=None, **kwargs):
            if env:
                captured_envs.append(env)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=capture_run,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={
                    "HOST": "10.0.0.42",
                    "CA_PATH": "../../certs/ca/ca.pem",
                },
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env",
                side_effect=lambda v: v,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            run(ctx)

        assert captured_envs[0]["CA_PATH"] == "../../certs/ca/ca.pem"

    def test_ca_path_aws_uses_amazon_root(self, tmp_path: Path):
        """AWS mode should use AmazonRootCA1 path."""
        ctx = _make_ctx(tmp_path, mode="aws")

        captured_envs: list[dict[str, str]] = []

        def capture_run(cmd, *, cwd=None, env=None, **kwargs):
            if env:
                captured_envs.append(env)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=capture_run,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={
                    "HOST": "supervictor.advin.io",
                    "CA_PATH": "../../certs/AmazonRootCA1.pem",
                },
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env",
                side_effect=lambda v: v,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            run(ctx)

        assert captured_envs[0]["CA_PATH"] == "../../certs/AmazonRootCA1.pem"

    def test_device_name_and_ca_path_both_present(self, tmp_path: Path):
        """Both DEVICE_NAME and CA_PATH must be in build env simultaneously."""
        ctx = _make_ctx(tmp_path)

        captured_envs: list[dict[str, str]] = []

        def capture_run(cmd, *, cwd=None, env=None, **kwargs):
            if env:
                captured_envs.append(env)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=capture_run,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={
                    "HOST": "10.0.0.42",
                    "CA_PATH": "../../certs/ca/ca.pem",
                },
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env",
                side_effect=lambda v: v,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            run(ctx)

        for env in captured_envs:
            assert "DEVICE_NAME" in env
            assert "CA_PATH" in env
            assert env["DEVICE_NAME"] == "test-device"

    def test_missing_ca_path_still_builds(self, tmp_path: Path):
        """If .env.dev omits CA_PATH, flash still runs (cargo may use .cargo/config.toml)."""
        ctx = _make_ctx(tmp_path)

        call_count = 0

        def counting_run(cmd, **kwargs):
            nonlocal call_count
            call_count += 1

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=counting_run,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={"HOST": "10.0.0.42"},
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env",
                side_effect=lambda v: v,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            result = run(ctx)

        # Should still proceed (cargo will fail at compile time if CA_PATH missing,
        # but the flash phase itself shouldn't gatekeep this)
        assert result.status == PhaseStatus.PASSED
        assert call_count == 2
