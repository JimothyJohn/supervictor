"""Tests for quickstart command handlers — dev, edge, staging, prod."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from quickstart.config import ProjectConfig


@pytest.fixture()
def config(tmp_path: Path) -> ProjectConfig:
    """Project config pointing at tmp_path with minimal .env files."""
    (tmp_path / ".env.dev").write_text("HOST=localhost\nSAM_LOCAL_PORT=3000\n")
    (tmp_path / ".env.staging").write_text("HOST=dev.example.com\nAPI_PATH=/dev/hello\n")
    (tmp_path / "device").mkdir()
    (tmp_path / "cloud").mkdir()
    return ProjectConfig.from_repo_root(tmp_path)


@pytest.fixture()
def dry_args() -> argparse.Namespace:
    """Common dry-run args."""
    return argparse.Namespace(verbose=False, dry_run=True, serve=False)


# ---------------------------------------------------------------------------
# qs dev
# ---------------------------------------------------------------------------


class TestDevCommand:
    """Tests for quickstart.commands.dev.run_dev."""

    @patch("quickstart.commands.dev.require")
    @patch("quickstart.commands.dev._rust_host_target", return_value="aarch64-apple-darwin")
    @patch("quickstart.runner.run")
    @patch("quickstart.runner.start_background", return_value=None)
    def test_dry_run_returns_zero(
        self,
        _mock_bg: MagicMock,
        _mock_run: MagicMock,
        _mock_target: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.dev import run_dev

        rc = run_dev(dry_args, config)
        assert rc == 0

    @patch("quickstart.commands.dev.require")
    @patch("quickstart.commands.dev._rust_host_target", return_value="aarch64-apple-darwin")
    @patch("quickstart.runner.run", side_effect=subprocess.CalledProcessError(1, "cargo"))
    def test_rust_test_failure_returns_one(
        self,
        _mock_run: MagicMock,
        _mock_target: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.dev import run_dev

        rc = run_dev(dry_args, config)
        assert rc == 1

    def test_rust_host_target_parses_output(self) -> None:
        from quickstart.commands.dev import _rust_host_target

        mock_output = "rustc 1.75.0\nhost: aarch64-apple-darwin\nrelease: 1.75.0\n"
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                [], 0, stdout=mock_output
            )
            target = _rust_host_target()
        assert target == "aarch64-apple-darwin"

    def test_rust_host_target_raises_on_missing(self) -> None:
        from quickstart.commands.dev import _rust_host_target

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = subprocess.CompletedProcess(
                [], 0, stdout="rustc 1.75.0\n"
            )
            with pytest.raises(RuntimeError, match="Cannot determine"):
                _rust_host_target()


# ---------------------------------------------------------------------------
# qs edge
# ---------------------------------------------------------------------------


class TestEdgeCommand:
    """Tests for quickstart.commands.edge.run_edge."""

    @patch("quickstart.commands.edge.require")
    @patch("quickstart.runner.run")
    def test_dry_run_returns_zero(
        self,
        _mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.edge import run_edge

        rc = run_edge(dry_args, config)
        assert rc == 0

    @patch("quickstart.commands.edge.require")
    @patch("quickstart.runner.run", side_effect=subprocess.CalledProcessError(1, "cargo"))
    def test_flash_failure_returns_one(
        self,
        _mock_run: MagicMock,
        _mock_require: MagicMock,
        dry_args: argparse.Namespace,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.edge import run_edge

        rc = run_edge(dry_args, config)
        assert rc == 1

    @patch("quickstart.commands.edge.require")
    @patch("quickstart.runner.run")
    def test_uses_espflash_port_from_env(
        self,
        mock_run: MagicMock,
        _mock_require: MagicMock,
        config: ProjectConfig,
        tmp_path: Path,
    ) -> None:
        from quickstart.commands.edge import run_edge

        (tmp_path / ".env.dev").write_text("ESPFLASH_PORT=/dev/ttyUSB0\n")
        args = argparse.Namespace(verbose=False, dry_run=True, serve=False)
        run_edge(args, config)


# ---------------------------------------------------------------------------
# qs staging
# ---------------------------------------------------------------------------


class TestStagingCommand:
    """Tests for quickstart.commands.staging."""

    @patch("quickstart.commands.staging.require")
    @patch("quickstart.commands.staging.dev.run_dev", return_value=1)
    def test_aborts_on_dev_gate_failure(
        self,
        _mock_dev: MagicMock,
        _mock_require: MagicMock,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.staging import run_staging

        args = argparse.Namespace(verbose=False, dry_run=True)
        rc = run_staging(args, config)
        assert rc == 1

    @patch("quickstart.commands.staging.require")
    @patch("quickstart.commands.staging._ensure_certs")
    @patch("quickstart.commands.staging.SamLocal")
    @patch("quickstart.runner.run")
    def test_skip_dev_gate(
        self,
        _mock_run: MagicMock,
        _mock_sam_cls: MagicMock,
        _mock_certs: MagicMock,
        _mock_require: MagicMock,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.staging import run_staging

        args = argparse.Namespace(verbose=False, dry_run=True)
        rc = run_staging(args, config, skip_dev_gate=True)
        assert rc == 0

    def test_ensure_certs_skips_existing(self, config: ProjectConfig, tmp_path: Path) -> None:
        from quickstart.commands.staging import _ensure_certs

        ca_dir = tmp_path / "certs" / "ca"
        ca_dir.mkdir(parents=True)
        (ca_dir / "ca.pem").write_text("fake")

        dev_dir = tmp_path / "certs" / "devices" / "test-device"
        dev_dir.mkdir(parents=True)
        (dev_dir / "client.pem").write_text("fake")

        with patch("quickstart.runner.run") as mock_run:
            _ensure_certs(config, {}, verbose=False, dry_run=False)
            mock_run.assert_not_called()


# ---------------------------------------------------------------------------
# qs prod
# ---------------------------------------------------------------------------


class TestProdCommand:
    """Tests for quickstart.commands.prod.run_prod."""

    @patch("quickstart.commands.prod.staging.run_staging", return_value=0)
    @patch("quickstart.commands.prod.dev.run_dev", return_value=0)
    @patch("quickstart.runner.confirm", return_value=False)
    def test_aborted_confirmation_returns_one(
        self,
        _mock_confirm: MagicMock,
        _mock_dev: MagicMock,
        _mock_staging: MagicMock,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.prod import run_prod

        args = argparse.Namespace(verbose=False, dry_run=True)
        rc = run_prod(args, config)
        assert rc == 1

    @patch("quickstart.commands.prod.dev.run_dev", return_value=1)
    def test_dev_gate_failure_aborts(
        self,
        _mock_dev: MagicMock,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.prod import run_prod

        args = argparse.Namespace(verbose=False, dry_run=True)
        rc = run_prod(args, config)
        assert rc != 0

    @patch("quickstart.commands.prod.staging.run_staging", return_value=1)
    @patch("quickstart.commands.prod.dev.run_dev", return_value=0)
    def test_staging_gate_failure_aborts(
        self,
        _mock_dev: MagicMock,
        _mock_staging: MagicMock,
        config: ProjectConfig,
    ) -> None:
        from quickstart.commands.prod import run_prod

        args = argparse.Namespace(verbose=False, dry_run=True)
        rc = run_prod(args, config)
        assert rc != 0


# ---------------------------------------------------------------------------
# __main__
# ---------------------------------------------------------------------------


class TestFindRepoRoot:
    """Tests for _find_repo_root."""

    def test_finds_repo_root(self) -> None:
        from quickstart.__main__ import _find_repo_root

        root = _find_repo_root()
        assert (root / ".git").exists()
