"""Unit tests for uplink handlers.

Tests pure business logic — no API Gateway event dicts, no Lambda runtime.
"""

from uplink.handlers import handle_hello, handle_uplink
from uplink.models import HelloResponse, UplinkResponse


# ---------------------------------------------------------------------------
# GET / — handle_hello
# ---------------------------------------------------------------------------


class TestHandleHello:
    """Tests for handle_hello (GET / logic)."""

    def test_returns_hello_response(self) -> None:
        result = handle_hello()
        assert isinstance(result, HelloResponse)

    def test_message_is_non_empty(self) -> None:
        result = handle_hello()
        assert isinstance(result.message, str)
        assert len(result.message) > 0

    def test_client_subject_omitted_by_default(self) -> None:
        result = handle_hello()
        assert result.client_subject is None

    def test_includes_client_subject_when_provided(self) -> None:
        result = handle_hello(client_subject="CN=device-001,O=Supervictor")
        assert result.client_subject == "CN=device-001,O=Supervictor"

    def test_exclude_none_omits_client_subject(self) -> None:
        """When serialized with exclude_none, client_subject must be absent."""
        result = handle_hello()
        dumped = result.model_dump(exclude_none=True)
        assert "client_subject" not in dumped


# ---------------------------------------------------------------------------
# POST / — handle_uplink
# ---------------------------------------------------------------------------


class TestHandleUplink:
    """Tests for handle_uplink (POST / logic)."""

    def test_valid_payload_returns_200(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert status == 200

    def test_valid_payload_returns_uplink_response(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert isinstance(result, UplinkResponse)

    def test_echoes_device_id(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert result.device_id == "1234567890"

    def test_echoes_current(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert result.current == 100

    def test_response_has_message(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert result.message == "Uplink received"

    def test_omits_client_subject_without_cert(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        dumped = result.model_dump(exclude_none=True)
        assert "client_subject" not in dumped

    def test_includes_client_subject_with_cert(self) -> None:
        result, status = handle_uplink(
            '{"id":"1234567890","current":100}',
            client_subject="CN=device-001,O=Supervictor",
        )
        assert result.client_subject == "CN=device-001,O=Supervictor"

    def test_missing_body_returns_400(self) -> None:
        result, status = handle_uplink(None)
        assert status == 400
        assert result["error"] == "Missing request body"

    def test_empty_body_returns_400(self) -> None:
        result, status = handle_uplink("")
        assert status == 400

    def test_whitespace_body_returns_400(self) -> None:
        result, status = handle_uplink("   ")
        assert status == 400

    def test_invalid_schema_returns_422(self) -> None:
        result, status = handle_uplink('{"bad": "payload"}')
        assert status == 422
        assert result["error"] == "Invalid payload"

    def test_malformed_json_returns_422(self) -> None:
        result, status = handle_uplink('{"id": "device-1"')
        assert status == 422
        assert result["error"] == "Invalid payload"
        assert isinstance(result["detail"], str)
