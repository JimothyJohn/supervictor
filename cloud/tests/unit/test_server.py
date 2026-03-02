"""Unit tests for the Starlette server routes."""

from starlette.testclient import TestClient

from uplink.app import app


client = TestClient(app)


class TestGetHello:
    """GET / — health check via Starlette."""

    def test_returns_200(self) -> None:
        response = client.get("/")
        assert response.status_code == 200

    def test_response_has_message(self) -> None:
        body = client.get("/").json()
        assert "message" in body
        assert isinstance(body["message"], str)
        assert len(body["message"]) > 0

    def test_content_type_is_json(self) -> None:
        response = client.get("/")
        assert "application/json" in response.headers["content-type"]

    def test_no_client_subject_without_cert(self) -> None:
        body = client.get("/").json()
        assert "client_subject" not in body


class TestPostUplink:
    """POST / — device uplink via Starlette."""

    def test_valid_payload_returns_200(self) -> None:
        response = client.post("/", json={"id": "device-1", "current": 42})
        assert response.status_code == 200

    def test_echoes_device_id(self) -> None:
        body = client.post("/", json={"id": "device-1", "current": 42}).json()
        assert body["device_id"] == "device-1"

    def test_echoes_current(self) -> None:
        body = client.post("/", json={"id": "device-1", "current": 42}).json()
        assert body["current"] == 42

    def test_response_has_message(self) -> None:
        body = client.post("/", json={"id": "device-1", "current": 42}).json()
        assert body["message"] == "Uplink received"

    def test_missing_body_returns_400(self) -> None:
        response = client.post("/")
        assert response.status_code == 400

    def test_invalid_schema_returns_422(self) -> None:
        response = client.post("/", json={"bad": "payload"})
        assert response.status_code == 422

    def test_malformed_json_returns_422(self) -> None:
        response = client.post(
            "/",
            content='{"id": "device-1"',
            headers={"content-type": "application/json"},
        )
        assert response.status_code == 422
