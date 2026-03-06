"""Tests for quickstart.commands.onboard_phases."""

from __future__ import annotations

import json
import subprocess
import urllib.error
from io import BytesIO
from pathlib import Path
from unittest.mock import MagicMock, patch

from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
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


# ── phase_preflight ───────────────────────────────────────────────────


class TestPreflight:
    def test_all_tools_present(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        # Create .env.dev so it passes the file check
        ctx.config.env_dev.touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools", return_value=[]
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=True,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED

    def test_missing_tools(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools",
                return_value=["espflash"],
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=True,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "espflash" in result.message

    def test_docker_not_running(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools", return_value=[]
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=False,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Docker" in result.message

    def test_env_dev_missing(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        # Do NOT create .env.dev

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools", return_value=[]
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=True,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert ".env.dev" in result.message

    def test_aws_mode_checks_sam(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="aws")
        ctx.config.env_dev.touch()

        captured_required = []

        def fake_check_tools(tools):
            captured_required.extend(tools)
            return []

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools",
                side_effect=fake_check_tools,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=True,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            run(ctx)

        assert "sam" in captured_required

    def test_aws_mode_checks_aws_cli(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="aws")
        ctx.config.env_dev.touch()

        captured_required = []

        def fake_check_tools(tools):
            captured_required.extend(tools)
            return []

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_tools",
                side_effect=fake_check_tools,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_preflight.check_docker_running",
                return_value=True,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_preflight import run

            run(ctx)

        assert "aws" in captured_required


# ── phase_certs ───────────────────────────────────────────────────────


class TestCerts:
    def test_all_certs_present_skips(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        certs_dir = tmp_path / "certs"
        (certs_dir / "ca").mkdir(parents=True)
        (certs_dir / "ca" / "ca.pem").touch()
        device_dir = certs_dir / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()

        with patch(
            "quickstart.commands.onboard_phases.phase_certs._extract_subject_dn",
            return_value="CN=test-device",
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.SKIPPED
        assert ctx.certs_dir == certs_dir

    def test_generates_ca_and_device(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with (
            patch("quickstart.commands.onboard_phases.phase_certs.runner.run") as mock_run,
            patch(
                "quickstart.commands.onboard_phases.phase_certs._extract_subject_dn",
                return_value="CN=test-device",
            ),
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        # Should have called gen_certs.sh for both CA and device
        assert mock_run.call_count == 2

    def test_script_failure(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with patch(
            "quickstart.commands.onboard_phases.phase_certs.runner.run",
            side_effect=subprocess.CalledProcessError(1, "gen_certs.sh"),
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Cert generation failed" in result.message

    def test_uploads_truststore_in_aws_mode(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="aws")
        certs_dir = tmp_path / "certs"
        (certs_dir / "ca").mkdir(parents=True)
        ca_pem = certs_dir / "ca" / "ca.pem"
        ca_pem.touch()
        device_dir = certs_dir / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_certs._extract_subject_dn",
                return_value="CN=test-device",
            ),
            patch("quickstart.commands.onboard_phases.phase_certs.runner.run") as mock_run,
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.SKIPPED
        # Should have called aws s3 cp
        mock_run.assert_called_once()
        args = mock_run.call_args[0][0]
        assert args[:3] == ["aws", "s3", "cp"]
        assert args[3] == str(ca_pem)
        assert args[4] == "s3://supervictor/truststore.pem"

    def test_skips_truststore_in_onprem_mode(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="onprem")
        certs_dir = tmp_path / "certs"
        (certs_dir / "ca").mkdir(parents=True)
        (certs_dir / "ca" / "ca.pem").touch()
        device_dir = certs_dir / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_certs._extract_subject_dn",
                return_value="CN=test-device",
            ),
            patch("quickstart.commands.onboard_phases.phase_certs.runner.run") as mock_run,
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.SKIPPED
        # No S3 upload in onprem mode
        mock_run.assert_not_called()

    def test_skips_truststore_in_dry_run(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="aws", dry_run=True)
        certs_dir = tmp_path / "certs"
        (certs_dir / "ca").mkdir(parents=True)
        (certs_dir / "ca" / "ca.pem").touch()
        device_dir = certs_dir / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_certs._extract_subject_dn",
                return_value="CN=test-device",
            ),
            patch("quickstart.commands.onboard_phases.phase_certs.runner.run") as mock_run,
        ):
            from quickstart.commands.onboard_phases.phase_certs import run

            result = run(ctx)

        assert result.status == PhaseStatus.SKIPPED
        # No S3 upload in dry-run
        mock_run.assert_not_called()


# ── phase_server ──────────────────────────────────────────────────────


class TestServer:
    def test_onprem_delegates_to_compose(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="onprem")

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.start_compose",
            return_value=PhaseResult(PhaseStatus.PASSED),
        ) as mock_compose:
            from quickstart.commands.onboard_phases.phase_server import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        mock_compose.assert_called_once_with(ctx)

    def test_onprem_compose_failure(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="onprem")

        with patch(
            "quickstart.commands.onboard_phases.phase_server_compose.start_compose",
            return_value=PhaseResult(PhaseStatus.FAILED, "Compose up failed"),
        ):
            from quickstart.commands.onboard_phases.phase_server import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Compose up failed" in result.message

    def test_aws_mode_uses_sam(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, mode="aws")
        mock_sam = MagicMock()
        mock_sam.url = "http://localhost:3000"
        mock_sam._proc = MagicMock()

        with patch(
            "quickstart.commands.onboard_phases.phase_server.SamLocal", return_value=mock_sam
        ):
            from quickstart.commands.onboard_phases.phase_server import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        mock_sam.build.assert_called_once()
        mock_sam.start.assert_called_once()
        mock_sam.wait_ready.assert_called_once()
        assert ctx.api_url == "http://localhost:3000"


# ── phase_register ────────────────────────────────────────────────────


def _mock_response(status: int, body: dict | None = None) -> MagicMock:
    """Build a mock urllib response."""
    resp = MagicMock()
    resp.status = status
    if body is not None:
        resp.read.return_value = json.dumps(body).encode()
    return resp


class TestRegister:
    def test_success(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"
        ctx.subject_dn = "CN=test-device"

        post_resp = _mock_response(201)
        get_resp = _mock_response(200, {"status": "active"})

        with patch(
            "quickstart.commands.onboard_phases.phase_register.urllib.request.urlopen"
        ) as mock_urlopen:
            mock_urlopen.side_effect = [post_resp, get_resp]

            from quickstart.commands.onboard_phases.phase_register import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED

    def test_post_returns_non_201(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"
        ctx.subject_dn = "CN=test-device"

        post_resp = _mock_response(200)

        with patch(
            "quickstart.commands.onboard_phases.phase_register.urllib.request.urlopen",
            return_value=post_resp,
        ):
            from quickstart.commands.onboard_phases.phase_register import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "expected 201" in result.message

    def test_post_http_error(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"
        ctx.subject_dn = "CN=test-device"

        with patch(
            "quickstart.commands.onboard_phases.phase_register.urllib.request.urlopen",
            side_effect=urllib.error.HTTPError(
                "url", 409, "Conflict", {}, BytesIO(b"already exists")
            ),
        ):
            from quickstart.commands.onboard_phases.phase_register import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "409" in result.message

    def test_device_not_active(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"
        ctx.subject_dn = "CN=test-device"

        post_resp = _mock_response(201)
        get_resp = _mock_response(200, {"status": "pending"})

        with patch(
            "quickstart.commands.onboard_phases.phase_register.urllib.request.urlopen"
        ) as mock_urlopen:
            mock_urlopen.side_effect = [post_resp, get_resp]

            from quickstart.commands.onboard_phases.phase_register import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "pending" in result.message

    def test_dry_run_skips(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, dry_run=True)
        ctx.api_url = "http://localhost:8000"

        from quickstart.commands.onboard_phases.phase_register import run

        result = run(ctx)
        assert result.status == PhaseStatus.PASSED


# ── phase_flash ───────────────────────────────────────────────────────


class TestFlash:
    def test_success(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with (
            patch("quickstart.commands.onboard_phases.phase_flash.runner.run") as mock_run,
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={"HOST": "supervictor.advin.io"},
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env", side_effect=lambda v: v
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        assert mock_run.call_count == 2
        # Both calls should have DEVICE_NAME in env
        for call in mock_run.call_args_list:
            assert call.kwargs["env"]["DEVICE_NAME"] == "test-device"

    def test_flash_failure(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_flash.runner.run",
                side_effect=subprocess.CalledProcessError(1, "cargo run"),
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.load_env",
                return_value={"HOST": "supervictor.advin.io"},
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_flash.make_env", side_effect=lambda v: v
            ),
        ):
            from quickstart.commands.onboard_phases.phase_flash import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "Flash failed" in result.message


# ── phase_verify ──────────────────────────────────────────────────────


class TestVerify:
    def test_uplinks_found(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"

        resp = MagicMock()
        resp.read.return_value = json.dumps([{"ts": 1, "payload": {}}]).encode()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                return_value=resp,
            ),
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={"HOST": "localhost:8000"},
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED

    def test_timeout_no_uplinks(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path)
        ctx.api_url = "http://localhost:8000"

        resp = MagicMock()
        resp.read.return_value = json.dumps([]).encode()

        call_count = 0

        def fake_monotonic():
            nonlocal call_count
            call_count += 1
            if call_count <= 1:
                return 0.0
            return 100.0

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                return_value=resp,
            ),
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.time.monotonic",
                side_effect=fake_monotonic,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={"HOST": "localhost:8000"},
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "No uplinks" in result.message

    def test_onprem_uses_env_host_port(self, tmp_path: Path):
        """In onprem mode, verify against HOST:PORT from .env.dev over plain HTTP."""
        ctx = _make_ctx(tmp_path, mode="onprem")
        ctx.api_url = "http://localhost:8000"

        resp = MagicMock()
        resp.read.return_value = json.dumps([{"ts": 1, "payload": {}}]).encode()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                return_value=resp,
            ) as mock_urlopen,
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={"HOST": "192.168.0.1", "PORT": "8000"},
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        call_args = mock_urlopen.call_args
        assert "http://192.168.0.1:8000" in call_args[0][0]
        assert call_args[1].get("context") is None

    def test_onprem_falls_back_to_api_url(self, tmp_path: Path):
        """In onprem mode without HOST/PORT in env, fall back to ctx.api_url."""
        ctx = _make_ctx(tmp_path, mode="onprem")
        ctx.api_url = "http://localhost:8000"

        resp = MagicMock()
        resp.read.return_value = json.dumps([{"ts": 1, "payload": {}}]).encode()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                return_value=resp,
            ) as mock_urlopen,
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={},
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        call_args = mock_urlopen.call_args
        assert "localhost:8000" in call_args[0][0]

    def test_polls_production_via_mtls(self, tmp_path: Path):
        """When device targets a remote host in aws mode, verify via mTLS."""
        ctx = _make_ctx(tmp_path, mode="aws")
        ctx.api_url = "http://localhost:3000"
        certs_dir = tmp_path / "certs"
        device_dir = certs_dir / "devices" / "test-device"
        device_dir.mkdir(parents=True)
        (device_dir / "client.pem").touch()
        (device_dir / "client.key").touch()
        ctx.certs_dir = certs_dir

        resp = MagicMock()
        resp.read.return_value = json.dumps([{"ts": 1, "payload": {}}]).encode()

        mock_ssl_ctx = MagicMock()

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                return_value=resp,
            ) as mock_urlopen,
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={"HOST": "supervictor.advin.io"},
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.ssl.create_default_context",
                return_value=mock_ssl_ctx,
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.PASSED
        # Should poll production endpoint, not local
        call_args = mock_urlopen.call_args
        assert "supervictor.advin.io" in call_args[0][0]
        assert call_args[1]["context"] is mock_ssl_ctx
        # SSL context should have loaded client cert
        mock_ssl_ctx.load_cert_chain.assert_called_once()

    def test_production_timeout_fails(self, tmp_path: Path):
        """When device targets production and no uplinks arrive, fail (don't skip)."""
        ctx = _make_ctx(tmp_path, mode="aws")
        ctx.api_url = "http://localhost:3000"
        ctx.certs_dir = tmp_path / "certs"

        call_count = 0

        def fake_monotonic():
            nonlocal call_count
            call_count += 1
            if call_count <= 1:
                return 0.0
            return 100.0

        with (
            patch(
                "quickstart.commands.onboard_phases.phase_verify.urllib.request.urlopen",
                side_effect=urllib.error.URLError("mTLS failed"),
            ),
            patch("quickstart.commands.onboard_phases.phase_verify.time.sleep"),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.time.monotonic",
                side_effect=fake_monotonic,
            ),
            patch(
                "quickstart.commands.onboard_phases.phase_verify.load_env",
                return_value={"HOST": "supervictor.advin.io"},
            ),
        ):
            from quickstart.commands.onboard_phases.phase_verify import run

            result = run(ctx)

        assert result.status == PhaseStatus.FAILED
        assert "No uplinks" in result.message

    def test_dry_run_skips(self, tmp_path: Path):
        ctx = _make_ctx(tmp_path, dry_run=True)
        ctx.api_url = "http://localhost:8000"

        from quickstart.commands.onboard_phases.phase_verify import run

        result = run(ctx)
        assert result.status == PhaseStatus.PASSED
