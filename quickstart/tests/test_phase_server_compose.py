"""Tests for quickstart.commands.onboard_phases.phase_server_compose."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from quickstart.commands.onboard_types import OnboardContext, PhaseStatus
from quickstart.config import ProjectConfig


def _make_ctx(tmp_path: Path, *, mode: str = "onprem", dry_run: bool = False) -> OnboardContext:
    config = ProjectConfig.from_repo_root(tmp_path)
    return OnboardContext(
        config=config,
        device_name="test-device",
        owner_id="owner-123",
        mode=mode,
        verbose=False,
        dry_run=dry_run,
    )


# ── detect_lan_ip ────────────────────────────────────────────────────


class TestDetectLanIp:
    def test_returns_ip_string(self):
        from quickstart.commands.onboard_phases.phase_server_compose import detect_lan_ip

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.socket.socket"
        ) as mock_sock:
            mock_instance = MagicMock()
            mock_sock.return_value.__enter__ = MagicMock(return_value=mock_instance)
            mock_sock.return_value.__exit__ = MagicMock(return_value=False)
            mock_instance.getsockname.return_value = ("192.168.1.42", 0)

            ip = detect_lan_ip()

        assert ip == "192.168.1.42"

    def test_raises_on_network_error(self):
        from quickstart.commands.onboard_phases.phase_server_compose import detect_lan_ip

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.socket.socket"
        ) as mock_sock:
            mock_instance = MagicMock()
            mock_sock.return_value.__enter__ = MagicMock(return_value=mock_instance)
            mock_sock.return_value.__exit__ = MagicMock(return_value=False)
            mock_instance.connect.side_effect = OSError("Network unreachable")

            with pytest.raises(OSError, match="Network unreachable"):
                detect_lan_ip()


# ── _ensure_server_cert ──────────────────────────────────────────────


class TestEnsureServerCert:
    def test_skips_if_cert_exists(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import _ensure_server_cert

        ctx = _make_ctx(tmp_path)
        server_dir = tmp_path / "certs" / "servers" / "caddy"
        server_dir.mkdir(parents=True)
        (server_dir / "server.pem").touch()
        (server_dir / "server.key").touch()

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.runner.run"
        ) as mock_run:
            result = _ensure_server_cert(ctx, "10.0.0.1")

        mock_run.assert_not_called()
        assert result == server_dir

    def test_generates_cert_when_missing(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import _ensure_server_cert

        ctx = _make_ctx(tmp_path)

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.runner.run"
        ) as mock_run:
            _ensure_server_cert(ctx, "10.0.0.44")

        mock_run.assert_called_once()
        args = mock_run.call_args[0][0]
        assert "server" in args
        assert "caddy" in args
        assert "10.0.0.44" in args


# ── start_compose ────────────────────────────────────────────────────


class TestStartCompose:
    def test_missing_compose_file(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        # No docker-compose.yml exists
        result = start_compose(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Missing" in result.message

    def test_lan_ip_detection_fails(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        (tmp_path / "cloud" / "docker-compose.yml").parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / "cloud" / "docker-compose.yml").touch()

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
            side_effect=OSError("no network"),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "LAN IP" in result.message

    def test_cert_generation_fails(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        (tmp_path / "cloud" / "docker-compose.yml").parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / "cloud" / "docker-compose.yml").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
                side_effect=RuntimeError("openssl not found"),
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "cert generation failed" in result.message.lower()

    def test_compose_up_fails(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        def _fail_on_up(cmd, **kwargs):
            if "up" in cmd:
                raise RuntimeError("docker not running")

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.runner.run",
                side_effect=_fail_on_up,
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Compose up failed" in result.message

    def test_tears_down_existing_stack_before_up(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path, dry_run=True)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        call_order: list[str] = []

        def _track_calls(cmd, **kwargs):
            if "down" in cmd:
                call_order.append("down")
            elif "up" in cmd:
                call_order.append("up")

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.runner.run",
                side_effect=_track_calls,
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.PASSED
        assert call_order == ["down", "up"]

    def test_down_failure_does_not_block_up(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path, dry_run=True)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        def _fail_on_down(cmd, **kwargs):
            if "down" in cmd:
                raise RuntimeError("no existing containers")

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.runner.run",
                side_effect=_fail_on_down,
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.PASSED

    def test_dry_run_skips_readiness_check(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path, dry_run=True)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch("quickstart.commands.onboard_phases.phase_server_compose.runner.run"),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.PASSED
        assert ctx.compose_file == compose_file
        assert ctx.api_url == "https://10.0.0.42"

    def test_success_with_readiness(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        # Create device certs for readiness check
        device_dir = tmp_path / "certs" / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()
        (device_dir / "client.key").touch()
        ca_dir = tmp_path / "certs" / "ca"
        ca_dir.mkdir(parents=True)
        (ca_dir / "ca.pem").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch("quickstart.commands.onboard_phases.phase_server_compose.runner.run"),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._wait_for_https",
                return_value=True,
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.PASSED
        assert ctx.compose_file == compose_file
        assert ctx.api_url == "https://10.0.0.42"

    def test_readiness_timeout(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import start_compose

        ctx = _make_ctx(tmp_path)
        compose_file = tmp_path / "cloud" / "docker-compose.yml"
        compose_file.parent.mkdir(parents=True, exist_ok=True)
        compose_file.touch()

        # Create device certs
        device_dir = tmp_path / "certs" / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()
        (device_dir / "client.key").touch()
        ca_dir = tmp_path / "certs" / "ca"
        ca_dir.mkdir(parents=True)
        (ca_dir / "ca.pem").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose.detect_lan_ip",
                return_value="10.0.0.42",
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._ensure_server_cert",
            ),
            patch("quickstart.commands.onboard_phases.phase_server_compose.runner.run"),
            patch(
                "quickstart.commands.onboard_phases.phase_server_compose._wait_for_https",
                return_value=False,
            ),
        ):
            result = start_compose(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "ready" in result.message.lower()


# ── stop_compose ─────────────────────────────────────────────────────


class TestStopCompose:
    def test_noop_when_no_compose_file(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import stop_compose

        ctx = _make_ctx(tmp_path)
        ctx.compose_file = None

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.runner.run"
        ) as mock_run:
            stop_compose(ctx)

        mock_run.assert_not_called()

    def test_runs_compose_down(self, tmp_path: Path):
        from quickstart.commands.onboard_phases.phase_server_compose import stop_compose

        ctx = _make_ctx(tmp_path)
        ctx.compose_file = tmp_path / "cloud" / "docker-compose.yml"

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.runner.run"
        ) as mock_run:
            stop_compose(ctx)

        mock_run.assert_called_once()
        args = mock_run.call_args[0][0]
        assert "docker" in args
        assert "compose" in args
        assert "down" in args
