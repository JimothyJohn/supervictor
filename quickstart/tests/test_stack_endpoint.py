"""Tests for SamLocal.stack_endpoint and _read_stack_name."""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from quickstart.config import ProjectConfig
from quickstart.sam import SamLocal


@pytest.fixture()
def config(tmp_path: Path) -> ProjectConfig:
    cloud_dir = tmp_path / "cloud"
    cloud_dir.mkdir()
    # Minimal samconfig.toml
    (cloud_dir / "samconfig.toml").write_text(
        '[dev]\n'
        '[dev.deploy.parameters]\n'
        'stack_name = "supervictor-dev"\n'
        '\n'
        '[prod]\n'
        '[prod.deploy.parameters]\n'
        'stack_name = "supervictor-prod"\n'
    )
    return ProjectConfig(
        repo_root=tmp_path,
        device_dir=tmp_path / "device",
        cloud_dir=cloud_dir,
        env_dev=tmp_path / ".env.dev",
        env_staging=tmp_path / ".env.staging",
    )


@pytest.fixture()
def sam(config: ProjectConfig) -> SamLocal:
    return SamLocal(config)


@pytest.fixture()
def sam_dry(config: ProjectConfig) -> SamLocal:
    return SamLocal(config, dry_run=True)


class TestReadStackName:
    """Tests for SamLocal._read_stack_name — samconfig.toml parsing."""

    def test_reads_dev_stack_name(self, sam: SamLocal) -> None:
        assert sam._read_stack_name("dev") == "supervictor-dev"

    def test_reads_prod_stack_name(self, sam: SamLocal) -> None:
        assert sam._read_stack_name("prod") == "supervictor-prod"

    def test_missing_config_env_raises(self, sam: SamLocal) -> None:
        with pytest.raises(RuntimeError, match="No stack_name"):
            sam._read_stack_name("nonexistent")

    def test_falls_back_to_global_parameters(self, tmp_path: Path) -> None:
        cloud_dir = tmp_path / "cloud2"
        cloud_dir.mkdir()
        (cloud_dir / "samconfig.toml").write_text(
            '[staging]\n'
            '[staging.global.parameters]\n'
            'stack_name = "my-staging-stack"\n'
        )
        cfg = ProjectConfig(
            repo_root=tmp_path,
            device_dir=tmp_path / "device",
            cloud_dir=cloud_dir,
            env_dev=tmp_path / ".env.dev",
            env_staging=tmp_path / ".env.staging",
        )
        sam = SamLocal(cfg)
        assert sam._read_stack_name("staging") == "my-staging-stack"


class TestStackEndpoint:
    """Tests for SamLocal.stack_endpoint — CF output query."""

    def test_dry_run_returns_placeholder(self, sam_dry: SamLocal) -> None:
        url = sam_dry.stack_endpoint("dev")
        assert url.startswith("https://")
        assert "dev" in url

    @patch("quickstart.runner.run")
    def test_returns_endpoint_from_cf_output(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 0,
            stdout="https://abc123.execute-api.us-east-1.amazonaws.com/dev/\n",
            stderr="",
        )
        url = sam.stack_endpoint("dev")
        assert url == "https://abc123.execute-api.us-east-1.amazonaws.com/dev"

    @patch("quickstart.runner.run")
    def test_strips_trailing_slash(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 0,
            stdout="https://host.amazonaws.com/dev/\n",
            stderr="",
        )
        url = sam.stack_endpoint("dev")
        assert not url.endswith("/")

    @patch("quickstart.runner.run")
    def test_empty_output_raises(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 0, stdout="\n", stderr=""
        )
        with pytest.raises(RuntimeError, match="No SupervictorApiEndpoint"):
            sam.stack_endpoint("dev")

    @patch("quickstart.runner.run")
    def test_calls_aws_cli_with_correct_stack_name(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 0,
            stdout="https://host.amazonaws.com/dev/\n",
            stderr="",
        )
        sam.stack_endpoint("dev")
        cmd = mock_run.call_args.args[0]
        assert "supervictor-dev" in cmd

    @patch("quickstart.runner.run")
    def test_prod_uses_prod_stack_name(
        self, mock_run: MagicMock, sam: SamLocal
    ) -> None:
        mock_run.return_value = subprocess.CompletedProcess(
            [], 0,
            stdout="https://host.amazonaws.com/prod/\n",
            stderr="",
        )
        sam.stack_endpoint("prod")
        cmd = mock_run.call_args.args[0]
        assert "supervictor-prod" in cmd
