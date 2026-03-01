"""Tests for quickstart.sam — SAM lifecycle management."""

from __future__ import annotations

import subprocess
import urllib.error
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from quickstart.config import ProjectConfig
from quickstart.sam import SamLocal


@pytest.fixture()
def config(tmp_path: Path) -> ProjectConfig:
    """Minimal project config for SAM tests."""
    return ProjectConfig(
        repo_root=tmp_path,
        device_dir=tmp_path / "device",
        cloud_dir=tmp_path / "cloud",
        env_dev=tmp_path / ".env.dev",
        env_staging=tmp_path / ".env.staging",
        sam_local_port=3999,
        sam_ready_timeout=2,
        sam_log_file=str(tmp_path / "sam.log"),
    )


@pytest.fixture()
def sam_dry(config: ProjectConfig) -> SamLocal:
    """SamLocal in dry-run mode — no subprocesses."""
    return SamLocal(config, dry_run=True)


@pytest.fixture()
def sam(config: ProjectConfig) -> SamLocal:
    """SamLocal with default settings (not dry-run)."""
    return SamLocal(config)


class TestSamLocalProperties:
    """Tests for SamLocal basic properties."""

    def test_url_includes_port(self, sam_dry: SamLocal) -> None:
        assert sam_dry.url == "http://localhost:3999"

    def test_url_uses_config_port(self, config: ProjectConfig) -> None:
        custom = SamLocal(
            ProjectConfig(
                repo_root=config.repo_root,
                device_dir=config.device_dir,
                cloud_dir=config.cloud_dir,
                env_dev=config.env_dev,
                env_staging=config.env_staging,
                sam_local_port=4567,
            )
        )
        assert custom.url == "http://localhost:4567"


class TestSamBuild:
    """Tests for SamLocal.build() — export deps + sam build."""

    def test_dry_run_does_not_execute(
        self, sam_dry: SamLocal, capsys: pytest.CaptureFixture[str]
    ) -> None:
        sam_dry.build()
        captured = capsys.readouterr()
        assert "[dry-run]" in captured.out

    @patch("quickstart.runner.run")
    def test_calls_uv_export(self, mock_run: MagicMock, sam: SamLocal) -> None:
        sam.build()
        calls = [c.args[0] for c in mock_run.call_args_list]
        assert any("uv" in cmd and "export" in cmd for cmd in calls)

    @patch("quickstart.runner.run")
    def test_calls_sam_build(self, mock_run: MagicMock, sam: SamLocal) -> None:
        sam.build()
        calls = [c.args[0] for c in mock_run.call_args_list]
        assert any("sam" in cmd and "build" in cmd for cmd in calls)


class TestSamStart:
    """Tests for SamLocal.start() — background process launch."""

    def test_dry_run_does_not_start_process(self, sam_dry: SamLocal) -> None:
        sam_dry.start()
        assert sam_dry._proc is None

    @patch("quickstart.runner.start_background")
    def test_starts_background_process(
        self, mock_bg: MagicMock, sam: SamLocal
    ) -> None:
        mock_bg.return_value = MagicMock()
        sam.start()
        mock_bg.assert_called_once()
        cmd = mock_bg.call_args.args[0]
        assert "sam" in cmd
        assert "start-api" in cmd


class TestSamWaitReady:
    """Tests for SamLocal.wait_ready() — HTTP health probe polling."""

    def test_dry_run_returns_immediately(
        self, sam_dry: SamLocal, capsys: pytest.CaptureFixture[str]
    ) -> None:
        sam_dry.wait_ready()
        captured = capsys.readouterr()
        assert "dry-run" in captured.out

    @patch("urllib.request.urlopen")
    def test_succeeds_on_http_200(
        self, mock_urlopen: MagicMock, sam: SamLocal
    ) -> None:
        mock_resp = MagicMock()
        mock_resp.status = 200
        mock_urlopen.return_value = mock_resp
        sam.wait_ready()

    @patch("urllib.request.urlopen")
    def test_succeeds_on_http_403(
        self, mock_urlopen: MagicMock, sam: SamLocal
    ) -> None:
        mock_urlopen.side_effect = urllib.error.HTTPError(
            "http://localhost", 403, "Forbidden", {}, None
        )
        sam.wait_ready()

    @patch("urllib.request.urlopen")
    def test_timeout_raises(
        self, mock_urlopen: MagicMock, sam: SamLocal, config: ProjectConfig
    ) -> None:
        mock_urlopen.side_effect = urllib.error.URLError("Connection refused")
        with pytest.raises(TimeoutError, match="did not start"):
            sam.wait_ready()


class TestSamStop:
    """Tests for SamLocal.stop() — process termination."""

    def test_stop_no_process_is_noop(self, sam: SamLocal) -> None:
        sam.stop()

    def test_stop_already_exited_is_noop(self, sam: SamLocal) -> None:
        mock_proc = MagicMock()
        mock_proc.poll.return_value = 0
        sam._proc = mock_proc
        sam.stop()
        mock_proc.terminate.assert_not_called()

    def test_stop_terminates_running_process(self, sam: SamLocal) -> None:
        mock_proc = MagicMock()
        mock_proc.poll.return_value = None
        sam._proc = mock_proc
        sam.stop()
        mock_proc.terminate.assert_called_once()

    def test_stop_kills_on_timeout(self, sam: SamLocal) -> None:
        mock_proc = MagicMock()
        mock_proc.poll.return_value = None
        # First call (with timeout=5) raises; second call (after kill) succeeds
        mock_proc.wait.side_effect = [subprocess.TimeoutExpired("sam", 5), None]
        sam._proc = mock_proc
        sam.stop()
        mock_proc.kill.assert_called_once()


class TestSamDeploy:
    """Tests for SamLocal.deploy() — sam deploy."""

    def test_dry_run_does_not_deploy(
        self, sam_dry: SamLocal, capsys: pytest.CaptureFixture[str]
    ) -> None:
        sam_dry.deploy("dev")
        captured = capsys.readouterr()
        assert "[dry-run]" in captured.out

    @patch("quickstart.runner.run")
    def test_deploy_success(self, mock_run: MagicMock, sam: SamLocal) -> None:
        mock_run.return_value = subprocess.CompletedProcess([], 0, stdout="", stderr="")
        sam.deploy("dev")
        cmd = mock_run.call_args.args[0]
        assert "deploy" in cmd
        assert "dev" in cmd

    @patch("quickstart.runner.run")
    def test_deploy_no_changes_treated_as_success(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 1, stdout="", stderr="No changes to deploy"
        )
        sam.deploy("dev")

    @patch("quickstart.runner.run")
    def test_deploy_failure_raises(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 1, stdout="", stderr="Error: stack failed"
        )
        with pytest.raises(subprocess.CalledProcessError):
            sam.deploy("dev")


class TestSamContextManager:
    """Tests for SamLocal as context manager."""

    @patch.object(SamLocal, "wait_ready")
    @patch.object(SamLocal, "start")
    @patch.object(SamLocal, "stop")
    def test_enter_starts_and_waits(
        self, mock_stop: MagicMock, mock_start: MagicMock, mock_wait: MagicMock,
        sam: SamLocal,
    ) -> None:
        with sam:
            mock_start.assert_called_once()
            mock_wait.assert_called_once()
        mock_stop.assert_called_once()

    @patch.object(SamLocal, "wait_ready")
    @patch.object(SamLocal, "start")
    @patch.object(SamLocal, "stop")
    def test_exit_stops_on_exception(
        self, mock_stop: MagicMock, mock_start: MagicMock, mock_wait: MagicMock,
        sam: SamLocal,
    ) -> None:
        with pytest.raises(ValueError):
            with sam:
                raise ValueError("boom")
        mock_stop.assert_called_once()
