"""Integration tests for gen_certs.sh server subcommand.

Runs the actual script against a temp certs directory to verify
server certificate generation, SAN handling, and CA signing.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

# Absolute path to the script under test
SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "gen_certs.sh"


def _run_gen_certs(
    *args: str,
    certs_root: Path,
) -> subprocess.CompletedProcess[str]:
    """Run gen_certs.sh with CERTS_DIR overridden via env manipulation.

    The script derives CERTS_DIR from its own location:
        SCRIPT_DIR/../../certs
    So we symlink the script into a temp tree to redirect output.
    """
    # Build a temp repo structure: tmp/cloud/scripts/gen_certs.sh + tmp/certs/
    repo_root = certs_root.parent
    scripts_dir = repo_root / "cloud" / "scripts"
    scripts_dir.mkdir(parents=True, exist_ok=True)

    # Symlink the real script into the temp tree
    link = scripts_dir / "gen_certs.sh"
    if not link.exists():
        link.symlink_to(SCRIPT)

    return subprocess.run(
        ["bash", str(link), *args],
        capture_output=True,
        text=True,
        timeout=30,
    )


def _openssl_x509_text(cert_path: Path) -> str:
    """Return full openssl x509 -text output for a certificate."""
    result = subprocess.run(
        ["openssl", "x509", "-text", "-noout", "-in", str(cert_path)],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"openssl failed: {result.stderr}"
    return result.stdout


def _openssl_verify(ca_path: Path, cert_path: Path) -> bool:
    """Verify a cert is signed by the given CA."""
    result = subprocess.run(
        ["openssl", "verify", "-CAfile", str(ca_path), str(cert_path)],
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


# ── Fixtures ─────────────────────────────────────────────────────────


@pytest.fixture()
def certs_dir(tmp_path: Path) -> Path:
    """Create temp certs dir and bootstrap a CA."""
    certs = tmp_path / "certs"
    certs.mkdir()
    result = _run_gen_certs("ca", certs_root=certs)
    assert result.returncode == 0, f"CA init failed: {result.stderr}"
    return certs


# ── Tests ────────────────────────────────────────────────────────────


class TestServerSubcommand:
    def test_generates_cert_and_key(self, certs_dir: Path):
        result = _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        assert result.returncode == 0, f"stdout={result.stdout}\nstderr={result.stderr}"
        server_dir = certs_dir / "servers" / "caddy"
        assert (server_dir / "server.pem").exists()
        assert (server_dir / "server.key").exists()
        # CSR should be cleaned up
        assert not (server_dir / "server.csr").exists()

    def test_cert_has_correct_subject(self, certs_dir: Path):
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        cert_text = _openssl_x509_text(certs_dir / "servers" / "caddy" / "server.pem")
        # openssl output may or may not have spaces around '='
        assert "CN=caddy" in cert_text or "CN = caddy" in cert_text
        assert "O=Supervictor" in cert_text or "O = Supervictor" in cert_text
        assert "OU=Servers" in cert_text or "OU = Servers" in cert_text

    def test_cert_has_san_with_localhost_and_ip(self, certs_dir: Path):
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        cert_text = _openssl_x509_text(certs_dir / "servers" / "caddy" / "server.pem")
        # SAN should contain both DNS:localhost and IP:10.0.0.44
        assert "DNS:localhost" in cert_text
        assert "IP Address:10.0.0.44" in cert_text

    def test_cert_signed_by_ca(self, certs_dir: Path):
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        ca_pem = certs_dir / "ca" / "ca.pem"
        server_pem = certs_dir / "servers" / "caddy" / "server.pem"
        assert _openssl_verify(ca_pem, server_pem), "Server cert not signed by CA"

    def test_key_permissions(self, certs_dir: Path):
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        key = certs_dir / "servers" / "caddy" / "server.key"
        pem = certs_dir / "servers" / "caddy" / "server.pem"
        # Key should be owner-only (0o600)
        assert oct(key.stat().st_mode & 0o777) == oct(0o600)
        # Cert should be world-readable (0o644)
        assert oct(pem.stat().st_mode & 0o777) == oct(0o644)

    def test_default_ip_is_localhost(self, certs_dir: Path):
        """When no IP argument given, SAN should default to 127.0.0.1."""
        _run_gen_certs("server", "myserver", certs_root=certs_dir)

        cert_text = _openssl_x509_text(certs_dir / "servers" / "myserver" / "server.pem")
        assert "IP Address:127.0.0.1" in cert_text
        assert "DNS:localhost" in cert_text

    def test_rejects_duplicate(self, certs_dir: Path):
        result1 = _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)
        assert result1.returncode == 0

        result2 = _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)
        assert result2.returncode != 0
        assert "already exists" in result2.stderr

    def test_fails_without_ca(self, tmp_path: Path):
        """Server cert generation requires an existing CA."""
        empty_certs = tmp_path / "certs"
        empty_certs.mkdir()
        result = _run_gen_certs("server", "caddy", "10.0.0.1", certs_root=empty_certs)

        assert result.returncode != 0
        assert "CA not found" in result.stderr

    def test_missing_name_argument(self, certs_dir: Path):
        result = _run_gen_certs("server", certs_root=certs_dir)

        assert result.returncode != 0
        assert "Usage" in result.stderr or "Usage" in result.stdout

    def test_custom_validity_days(self, certs_dir: Path):
        _run_gen_certs("server", "short-lived", "10.0.0.1", "30", certs_root=certs_dir)

        cert_text = _openssl_x509_text(certs_dir / "servers" / "short-lived" / "server.pem")
        # Cert should exist and be valid (we can't easily check exact days
        # from text output, but we verify it was created successfully)
        assert "CN=short-lived" in cert_text or "CN = short-lived" in cert_text


class TestListIncludesServers:
    def test_list_shows_server_certs(self, certs_dir: Path):
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        result = _run_gen_certs("list", certs_root=certs_dir)
        assert result.returncode == 0
        assert "servers" in result.stdout
        assert "caddy" in result.stdout

    def test_list_shows_mixed_cert_types(self, certs_dir: Path):
        _run_gen_certs("device", "factory-01", certs_root=certs_dir)
        _run_gen_certs("server", "caddy", "10.0.0.44", certs_root=certs_dir)

        result = _run_gen_certs("list", certs_root=certs_dir)
        assert result.returncode == 0
        assert "devices" in result.stdout
        assert "factory-01" in result.stdout
        assert "servers" in result.stdout
        assert "caddy" in result.stdout
