"""Unit tests for the /health endpoint."""

from starlette.testclient import TestClient
from uplink.app import app

client = TestClient(app)


class TestHealth:
    def test_health_returns_200(self) -> None:
        resp = client.get("/health")
        assert resp.status_code == 200

    def test_health_body(self) -> None:
        resp = client.get("/health")
        assert resp.json() == {"status": "ok"}
