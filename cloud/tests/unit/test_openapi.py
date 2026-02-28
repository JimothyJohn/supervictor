"""Tests that the OpenAPI spec is valid and matches handler behavior.

Validates that:
  - The spec is structurally valid OpenAPI 3.1.0
  - All Pydantic models are represented
  - GET and POST responses conform to their declared schemas
"""

import json
from typing import Any

import pytest

from hello_world import app


@pytest.fixture()
def spec() -> dict[str, Any]:
    return app.openapi_spec()


class TestOpenApiStructure:
    """Validate the OpenAPI spec structure."""

    def test_openapi_version(self, spec: dict[str, Any]) -> None:
        assert spec["openapi"] == "3.1.0"

    def test_info_title(self, spec: dict[str, Any]) -> None:
        assert spec["info"]["title"] == "Supervictor API"

    def test_info_version(self, spec: dict[str, Any]) -> None:
        assert spec["info"]["version"] == "0.1.0"

    def test_hello_path_exists(self, spec: dict[str, Any]) -> None:
        assert "/hello" in spec["paths"]

    def test_get_method_exists(self, spec: dict[str, Any]) -> None:
        assert "get" in spec["paths"]["/hello"]

    def test_post_method_exists(self, spec: dict[str, Any]) -> None:
        assert "post" in spec["paths"]["/hello"]


class TestGetHelloSchema:
    """Validate GET /hello schema matches HelloResponse."""

    def test_get_has_200_response(self, spec: dict[str, Any]) -> None:
        assert "200" in spec["paths"]["/hello"]["get"]["responses"]

    def test_get_200_schema_has_message_property(self, spec: dict[str, Any]) -> None:
        schema = spec["paths"]["/hello"]["get"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        assert "message" in schema["properties"]

    def test_get_200_schema_has_client_subject_property(
        self, spec: dict[str, Any]
    ) -> None:
        schema = spec["paths"]["/hello"]["get"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        assert "client_subject" in schema["properties"]

    def test_get_200_message_is_required(self, spec: dict[str, Any]) -> None:
        schema = spec["paths"]["/hello"]["get"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        assert "message" in schema["required"]

    def test_get_response_conforms_to_schema(self, spec: dict[str, Any]) -> None:
        """Actual GET handler output must include all required fields from spec."""
        schema = spec["paths"]["/hello"]["get"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        event = {
            "httpMethod": "GET",
            "path": "/hello",
            "requestContext": {"identity": {}},
        }
        result = app.lambda_handler(event, None)
        body = json.loads(result["body"])
        for field in schema["required"]:
            assert field in body, f"Required field '{field}' missing from GET response"


class TestPostUplinkSchema:
    """Validate POST /hello schema matches UplinkMessage / UplinkResponse."""

    def test_post_request_body_is_required(self, spec: dict[str, Any]) -> None:
        assert spec["paths"]["/hello"]["post"]["requestBody"]["required"] is True

    def test_post_request_schema_has_id(self, spec: dict[str, Any]) -> None:
        schema = spec["paths"]["/hello"]["post"]["requestBody"]["content"][
            "application/json"
        ]["schema"]
        assert "id" in schema["properties"]

    def test_post_request_schema_has_current(self, spec: dict[str, Any]) -> None:
        schema = spec["paths"]["/hello"]["post"]["requestBody"]["content"][
            "application/json"
        ]["schema"]
        assert "current" in schema["properties"]

    def test_post_request_id_and_current_are_required(
        self, spec: dict[str, Any]
    ) -> None:
        schema = spec["paths"]["/hello"]["post"]["requestBody"]["content"][
            "application/json"
        ]["schema"]
        assert "id" in schema["required"]
        assert "current" in schema["required"]

    def test_post_200_schema_has_device_id(self, spec: dict[str, Any]) -> None:
        schema = spec["paths"]["/hello"]["post"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        assert "device_id" in schema["properties"]

    def test_post_has_400_response(self, spec: dict[str, Any]) -> None:
        assert "400" in spec["paths"]["/hello"]["post"]["responses"]

    def test_post_has_422_response(self, spec: dict[str, Any]) -> None:
        assert "422" in spec["paths"]["/hello"]["post"]["responses"]

    def test_post_response_conforms_to_schema(self, spec: dict[str, Any]) -> None:
        """Actual POST handler output must include all required fields from spec."""
        schema = spec["paths"]["/hello"]["post"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]
        event = {
            "httpMethod": "POST",
            "path": "/hello",
            "body": '{"id":"test-123","current":42}',
            "requestContext": {"identity": {}},
        }
        result = app.lambda_handler(event, None)
        body = json.loads(result["body"])
        for field in schema["required"]:
            assert field in body, f"Required field '{field}' missing from POST response"
