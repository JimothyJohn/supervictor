"""Structural validation tests for docker-compose.yml and Caddyfile.

These verify the config files are well-formed and contain the expected
service definitions, volume mounts, and TLS directives — catching
typos and structural regressions without needing Docker running.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

CLOUD_DIR = Path(__file__).resolve().parents[2]
COMPOSE_FILE = CLOUD_DIR / "docker-compose.yml"
CADDYFILE = CLOUD_DIR / "Caddyfile"


@pytest.fixture()
def compose_text() -> str:
    return COMPOSE_FILE.read_text()


@pytest.fixture()
def caddy_text() -> str:
    return CADDYFILE.read_text()


# ── docker-compose.yml ───────────────────────────────────────────────


class TestDockerCompose:
    def test_file_exists(self):
        assert COMPOSE_FILE.exists()

    def test_defines_uplink_service(self, compose_text: str):
        assert "uplink:" in compose_text

    def test_defines_caddy_service(self, compose_text: str):
        assert "caddy:" in compose_text

    def test_uplink_uses_sqlite_backend(self, compose_text: str):
        assert "STORE_BACKEND: sqlite" in compose_text
        assert "SQLITE_DB_PATH" in compose_text

    def test_uplink_exposes_8000_not_published(self, compose_text: str):
        """Uplink should only expose (not publish) port 8000."""
        # 'expose' makes port available to linked services only
        assert "expose:" in compose_text
        assert '"8000"' in compose_text
        # Verify 8000 is NOT in the ports section (that would publish it)
        # The ports section should only have 443
        ports_match = re.search(r'ports:\s*\n\s*-\s*"(\d+):\d+"', compose_text)
        assert ports_match is not None
        assert ports_match.group(1) == "443"

    def test_caddy_publishes_443(self, compose_text: str):
        assert '"443:443"' in compose_text

    def test_caddy_mounts_certs_readonly(self, compose_text: str):
        assert "ca.pem:/etc/caddy/certs/ca.pem:ro" in compose_text
        assert "server.pem:/etc/caddy/certs/server.pem:ro" in compose_text
        assert "server.key:/etc/caddy/certs/server.key:ro" in compose_text

    def test_caddy_mounts_caddyfile(self, compose_text: str):
        assert "Caddyfile:/etc/caddy/Caddyfile:ro" in compose_text

    def test_caddy_depends_on_uplink_healthy(self, compose_text: str):
        assert "condition: service_healthy" in compose_text

    def test_uplink_has_healthcheck(self, compose_text: str):
        assert "healthcheck:" in compose_text
        assert "/health" in compose_text

    def test_caddy_uses_alpine_image(self, compose_text: str):
        assert "caddy:2-alpine" in compose_text

    def test_uplink_builds_from_uplink_dir(self, compose_text: str):
        assert "build: ./uplink" in compose_text


# ── Caddyfile ────────────────────────────────────────────────────────


class TestCaddyfile:
    def test_file_exists(self):
        assert CADDYFILE.exists()

    def test_auto_https_disabled(self, caddy_text: str):
        assert "auto_https off" in caddy_text

    def test_strict_sni_host_disabled(self, caddy_text: str):
        """IP-based access requires disabling strict SNI-to-Host matching."""
        assert "strict_sni_host insecure_off" in caddy_text

    def test_listens_on_443(self, caddy_text: str):
        assert ":443" in caddy_text

    def test_tls_with_server_cert_paths(self, caddy_text: str):
        assert "/etc/caddy/certs/server.pem" in caddy_text
        assert "/etc/caddy/certs/server.key" in caddy_text

    def test_mtls_require_and_verify(self, caddy_text: str):
        assert "require_and_verify" in caddy_text

    def test_mtls_trusted_ca(self, caddy_text: str):
        assert "trust_pool file" in caddy_text
        assert "pem_file /etc/caddy/certs/ca.pem" in caddy_text

    def test_reverse_proxy_to_uplink(self, caddy_text: str):
        assert "reverse_proxy uplink:8000" in caddy_text

    def test_forwards_client_subject_dn(self, caddy_text: str):
        assert "X-SSL-Client-Subject-DN" in caddy_text
        assert "{http.request.tls.client.subject}" in caddy_text

    def test_cert_paths_match_compose_mounts(self, compose_text: str, caddy_text: str):
        """Caddyfile cert paths must match the container-side mount targets."""
        # Extract container-side paths from compose volume mounts
        mount_re = re.compile(r":\s*(/etc/caddy/certs/\S+):ro")
        compose_paths = set(mount_re.findall(compose_text))

        # Extract file paths referenced in the Caddyfile
        caddy_paths = set(re.findall(r"/etc/caddy/certs/\S+", caddy_text))

        # Every path in the Caddyfile should have a matching mount
        assert caddy_paths, "No cert paths found in Caddyfile"
        assert caddy_paths.issubset(compose_paths), (
            f"Caddyfile references paths not mounted in compose: {caddy_paths - compose_paths}"
        )
