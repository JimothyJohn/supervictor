"""Project-specific configuration. Change this file for a new project."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ProjectConfig:
    repo_root: Path
    cloud_dir: Path
    env_dev: Path
    env_staging: Path

    # SAM local
    sam_local_port: int = 3000
    sam_ready_timeout: int = 120
    sam_log_file: str = "/tmp/supervictor_sam_local.log"
    sam_pid_file: str = "/tmp/supervictor_sam_local.pid"
    health_path: str = "/hello"

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
        cloud = root / "supervictor-cloud"
        return cls(
            repo_root=root,
            cloud_dir=cloud,
            env_dev=root / ".env.dev",
            env_staging=root / ".env.staging",
        )
