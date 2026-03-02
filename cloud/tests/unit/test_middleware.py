"""Unit tests for client certificate middleware."""

import json

from starlette.testclient import TestClient

from uplink.middleware import extract_client_subject
from uplink.app import app


client = TestClient(app)


class TestExtractClientSubjectViaLWA:
    """Cert extraction from Lambda Web Adapter x-amzn-request-context header."""

    def test_extracts_subject_from_request_context(self) -> None:
        ctx = {
            "identity": {
                "clientCert": {
                    "subjectDN": "CN=device-001,O=Supervictor",
                }
            }
        }
        response = client.get(
            "/", headers={"x-amzn-request-context": json.dumps(ctx)}
        )
        assert response.status_code == 200
        assert response.json()["client_subject"] == "CN=device-001,O=Supervictor"

    def test_returns_none_for_malformed_context(self) -> None:
        response = client.get(
            "/", headers={"x-amzn-request-context": "not-json"}
        )
        assert response.status_code == 200
        assert "client_subject" not in response.json()

    def test_returns_none_for_missing_cert_in_context(self) -> None:
        ctx = {"identity": {}}
        response = client.get(
            "/", headers={"x-amzn-request-context": json.dumps(ctx)}
        )
        assert response.status_code == 200
        assert "client_subject" not in response.json()


class TestExtractClientSubjectViaProxy:
    """Cert extraction from reverse proxy header."""

    def test_extracts_subject_from_ssl_header(self) -> None:
        response = client.get(
            "/",
            headers={"x-ssl-client-subject-dn": "CN=device-002,O=Supervictor"},
        )
        assert response.status_code == 200
        assert response.json()["client_subject"] == "CN=device-002,O=Supervictor"

    def test_lwa_header_takes_priority_over_proxy(self) -> None:
        ctx = {
            "identity": {
                "clientCert": {
                    "subjectDN": "CN=from-lwa,O=Supervictor",
                }
            }
        }
        response = client.get(
            "/",
            headers={
                "x-amzn-request-context": json.dumps(ctx),
                "x-ssl-client-subject-dn": "CN=from-proxy,O=Supervictor",
            },
        )
        assert response.status_code == 200
        assert response.json()["client_subject"] == "CN=from-lwa,O=Supervictor"


class TestExtractClientSubjectNone:
    """No cert headers — local dev behavior."""

    def test_returns_none_without_headers(self) -> None:
        response = client.get("/")
        assert response.status_code == 200
        assert "client_subject" not in response.json()
