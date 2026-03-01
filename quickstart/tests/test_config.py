"""Tests for quickstart.config — ProjectConfig dataclass."""

from __future__ import annotations

from pathlib import Path

from quickstart.config import ProjectConfig


class TestProjectConfig:
    """Tests for ProjectConfig.from_repo_root factory."""

    def test_from_repo_root_sets_device_dir(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.device_dir == tmp_path / "device"

    def test_from_repo_root_sets_cloud_dir(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.cloud_dir == tmp_path / "cloud"

    def test_from_repo_root_sets_env_dev(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.env_dev == tmp_path / ".env.dev"

    def test_from_repo_root_sets_env_staging(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.env_staging == tmp_path / ".env.staging"

    def test_default_sam_local_port(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.sam_local_port == 3000

    def test_default_sam_ready_timeout(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.sam_ready_timeout == 120

    def test_default_sam_config_envs(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.sam_config_env_dev == "dev"
        assert config.sam_config_env_prod == "prod"

    def test_frozen_dataclass_immutable(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        with __import__("pytest").raises(AttributeError):
            config.sam_local_port = 9999  # type: ignore[misc]

    def test_prod_api_endpoint(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.prod_api_endpoint == "https://supervictor.advin.io"

    def test_repo_root_stored(self, tmp_path: Path) -> None:
        config = ProjectConfig.from_repo_root(tmp_path)
        assert config.repo_root == tmp_path
