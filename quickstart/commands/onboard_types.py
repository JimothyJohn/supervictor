"""Types for the onboard command."""

from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from subprocess import Popen

from quickstart.config import ProjectConfig


class PhaseStatus(Enum):
    PASSED = "passed"
    FAILED = "failed"
    SKIPPED = "skipped"


@dataclass
class PhaseResult:
    status: PhaseStatus
    message: str = ""


@dataclass
class OnboardContext:
    config: ProjectConfig
    device_name: str
    owner_id: str
    mode: str  # "onprem" | "aws"
    verbose: bool
    dry_run: bool
    # Populated by phases:
    certs_dir: Path | None = None
    subject_dn: str | None = None
    api_url: str | None = None
    api_process: Popen | None = None
    compose_file: Path | None = None
