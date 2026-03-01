"""Tests for quickstart.commands.portal.run_portal."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, call, patch

import pytest

from quickstart.config import ProjectConfig


@pytest.fixture()
def config(tmp_path: Path) -> ProjectConfig:
    """Project config pointing at tmp_path."""
    (tmp_path / "device").mkdir()
    (tmp_path / "cloud").mkdir()
    (tmp_path / "portal").mkdir()
    (tmp_path / ".env.dev").write_text("")
    (tmp_path / ".env.staging").write_text("")
    return ProjectConfig.from_repo_root(tmp_path)


@pytest.fixture()
def dry_args() -> argparse.Namespace:
    return argparse.Namespace(verbose=False, dry_run=True)


class TestPortalCommand:
    """Tests for quickstart.commands.portal.run_portal."""

    @patch("quickstart.commands.portal.require")
    @patch("quickstart.runner.run")
    def test_dry_run_returns_zero(
        self,
        _mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.portal import run_portal

        rc = run_portal(dry_args, config)
        assert rc == 0

    @patch("quickstart.commands.portal.require")
    @patch("quickstart.runner.run")
    def test_runs_two_commands_in_order(
        self,
        mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.portal import run_portal

        run_portal(dry_args, config)
        assert mock_run.call_count == 2

        # First call: WASM build
        first_cmd = mock_run.call_args_list[0]
        assert first_cmd[0][0] == ["bash", "build.sh"]
        assert first_cmd[1]["cwd"] == config.repo_root / "portal"

        # Second call: firmware flash
        second_cmd = mock_run.call_args_list[1]
        assert "supervictor-portal" in second_cmd[0][0]
        assert "--features" in second_cmd[0][0]
        assert "portal" in second_cmd[0][0]
        assert second_cmd[1]["cwd"] == config.device_dir

    @patch("quickstart.commands.portal.require")
    @patch("quickstart.runner.run")
    def test_firmware_uses_portal_feature(
        self,
        mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.portal import run_portal

        run_portal(dry_args, config)
        firmware_cmd = mock_run.call_args_list[1][0][0]
        assert firmware_cmd == [
            "cargo", "run", "--bin", "supervictor-portal", "--features", "portal",
        ]

    @patch("quickstart.commands.portal.require")
    @patch(
        "quickstart.runner.run",
        side_effect=subprocess.CalledProcessError(1, "bash"),
    )
    def test_wasm_failure_returns_one_and_skips_firmware(
        self,
        mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.portal import run_portal

        rc = run_portal(dry_args, config)
        assert rc == 1
        # Only the WASM build was attempted
        assert mock_run.call_count == 1

    @patch("quickstart.commands.portal.require")
    @patch("quickstart.runner.run")
    def test_firmware_failure_returns_one(
        self,
        mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.portal import run_portal

        # First call (WASM build) succeeds, second (firmware) fails
        mock_run.side_effect = [
            None,
            subprocess.CalledProcessError(1, "cargo"),
        ]
        rc = run_portal(dry_args, config)
        assert rc == 1
        assert mock_run.call_count == 2

    def test_preflight_checks_correct_tools(self, config: ProjectConfig) -> None:
        from quickstart.commands.portal import run_portal

        dry_args = argparse.Namespace(verbose=False, dry_run=True)
        with (
            patch("quickstart.commands.portal.require") as mock_require,
            patch("quickstart.runner.run"),
        ):
            run_portal(dry_args, config)
            mock_require.assert_called_once_with(
                ["cargo", "espflash", "wasm-bindgen", "wasm-opt"]
            )
