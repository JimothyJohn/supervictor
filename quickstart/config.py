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

    # Logs — set dynamically in from_repo_root()
    log_dir: Path = Path(".logs")

    # SAM local — set dynamically in from_repo_root()
    sam_local_port: int = 3000
    sam_ready_timeout: int = 120
    sam_log_file: str = ".logs/sam_local.log"
    sam_pid_file: str = ".logs/sam_local.pid"

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
        log_dir = root / ".logs"
        return cls(
            repo_root=root,
            device_dir=root / "device",
            cloud_dir=root / "cloud",
            env_dev=root / ".env.dev",
            env_staging=root / ".env.staging",
            log_dir=log_dir,
            sam_log_file=str(log_dir / "sam_local.log"),
            sam_pid_file=str(log_dir / "sam_local.pid"),
        )
