"""Project-specific configuration. Change this file for a new project."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ProjectConfig:
    repo_root: Path
    device_dir: Path
    cloud_dir: Path
    env_dev: Path
    env_staging: Path

    # SAM local
    sam_local_port: int = 3000
    sam_ready_timeout: int = 120
    sam_log_file: str = "/tmp/supervictor_sam_local.log"
    sam_pid_file: str = "/tmp/supervictor_sam_local.pid"

    # SAM deploy config-env names (match samconfig.toml sections)
    sam_config_env_dev: str = "dev"
    sam_config_env_prod: str = "prod"

    # Cert generation
    certs_dir_name: str = "certs"
    gen_certs_script: str = "scripts/gen_certs.sh"

    # Prod endpoint for remote tests
    prod_api_endpoint: str = "https://supervictor.advin.io"

    @classmethod
    def from_repo_root(cls, root: Path) -> ProjectConfig:
        return cls(
            repo_root=root,
            device_dir=root / "device",
            cloud_dir=root / "cloud",
            env_dev=root / ".env.dev",
            env_staging=root / ".env.staging",
        )
