"""Shared fixtures for Supervictor integration tests.

Environment variables:
  SAM_LOCAL_URL   Base URL for sam local (e.g. http://localhost:3000).
                  Set by scripts/run_integration_tests.sh.
  API_ENDPOINT    Base URL for a deployed stack (e.g. https://supervictor.advin.io).
                  Set manually for remote/mTLS tests.
  TEST_CERT_DIR   Path to the certs/ directory (default: "certs").
                  Set by scripts/run_integration_tests.sh.
"""

import os
from pathlib import Path

import pytest


@pytest.fixture(scope="session")
def sam_local_url() -> str:
    """Base URL for sam local start-api."""
    url = os.environ.get("SAM_LOCAL_URL", "")
    if not url:
        pytest.skip("SAM_LOCAL_URL not set — run ./scripts/run_integration_tests.sh")
    return url.rstrip("/")


@pytest.fixture(scope="session")
def remote_api_url() -> str:
    """Base URL for the deployed HTTPS endpoint (with mTLS)."""
    url = os.environ.get("API_ENDPOINT", "")
    if not url:
        pytest.skip("API_ENDPOINT not set — deployed mTLS tests skipped")
    return url.rstrip("/")


@pytest.fixture(scope="session")
def cert_dir() -> Path:
    """Root of the certs/ directory."""
    return Path(os.environ.get("TEST_CERT_DIR", "certs"))


@pytest.fixture(scope="session")
def ca_cert_path(cert_dir: Path) -> Path:
    """Path to the CA certificate (PEM)."""
    path = cert_dir / "ca" / "ca.pem"
    if not path.exists():
        pytest.skip(f"CA cert not found at {path} — run: ./scripts/gen_certs.sh ca")
    return path


@pytest.fixture(scope="session")
def test_device_cert(cert_dir: Path) -> tuple[str, str]:
    """Return (cert_path, key_path) for the CI test device."""
    cert = cert_dir / "devices" / "test-device" / "client.pem"
    key = cert_dir / "devices" / "test-device" / "client.key"
    if not cert.exists():
        pytest.skip(
            f"Test device cert not found at {cert} — run: ./scripts/gen_certs.sh device test-device"
        )
    return (str(cert), str(key))
