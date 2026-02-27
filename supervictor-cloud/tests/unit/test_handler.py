"""Unit tests for hello_world Lambda handler.

Follows TDD Red-Green-Refactor cycle. Tests are written before implementation.
mTLS enforcement is at the API Gateway/domain level; handler tests validate
response structure and cert context extraction.
"""

import json
from typing import Any

import pytest

from hello_world import app

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture()
def apigw_event_with_cert() -> dict[str, Any]:
    """API Gateway proxy event including a mTLS client certificate context."""
    return {
        "httpMethod": "GET",
        "path": "/hello",
        "queryStringParameters": None,
        "headers": {
            "Accept": "application/json",
            "Host": "supervictor.advin.io",
        },
        "body": None,
        "isBase64Encoded": False,
        "requestContext": {
            "resourceId": "abc123",
            "apiId": "xyz789",
            "resourcePath": "/hello",
            "httpMethod": "GET",
            "requestId": "test-request-id-001",
            "stage": "prod",
            "identity": {
                "clientCert": {
                    "clientCertPem": (
                        "-----BEGIN CERTIFICATE-----\nMIIBxxx\n-----END CERTIFICATE-----"
                    ),
                    "subjectDN": "CN=device-001,O=Supervictor",
                    "issuerDN": "CN=SupervictorCA,O=Supervictor",
                    "serialNumber": "1",
                    "validity": {
                        "notBefore": "Jan 1 00:00:00 2024 GMT",
                        "notAfter": "Dec 31 23:59:59 2099 GMT",
                    },
                },
                "sourceIp": "10.0.0.1",
                "userAgent": "SupervictorDevice/1.0",
            },
        },
    }


@pytest.fixture()
def apigw_post_event() -> dict[str, Any]:
    """API Gateway POST event with a valid UplinkMessage body."""
    return {
        "httpMethod": "POST",
        "path": "/hello",
        "queryStringParameters": None,
        "headers": {
            "Content-Type": "application/json",
            "Host": "supervictor.advin.io",
        },
        "body": '{"id":"1234567890","current":100}',
        "isBase64Encoded": False,
        "requestContext": {
            "resourceId": "abc123",
            "apiId": "xyz789",
            "resourcePath": "/hello",
            "httpMethod": "POST",
            "requestId": "test-request-id-003",
            "stage": "dev",
            "identity": {
                "sourceIp": "10.0.0.1",
                "userAgent": "Uplink/0.1.0 (Platform; ESP32-C3)",
            },
        },
    }


@pytest.fixture()
def apigw_post_event_with_cert(apigw_post_event: dict[str, Any]) -> dict[str, Any]:
    """POST event with mTLS client certificate context."""
    event = apigw_post_event.copy()
    event["requestContext"] = {
        **event["requestContext"],
        "identity": {
            **event["requestContext"]["identity"],
            "clientCert": {
                "clientCertPem": "-----BEGIN CERTIFICATE-----\nMIIBxxx\n-----END CERTIFICATE-----",
                "subjectDN": "CN=device-001,O=Supervictor",
                "issuerDN": "CN=SupervictorCA,O=Supervictor",
                "serialNumber": "1",
                "validity": {
                    "notBefore": "Jan 1 00:00:00 2024 GMT",
                    "notAfter": "Dec 31 23:59:59 2099 GMT",
                },
            },
        },
    }
    return event


@pytest.fixture()
def apigw_event_no_cert() -> dict[str, Any]:
    """API Gateway proxy event without a client certificate (local/dev testing)."""
    return {
        "httpMethod": "GET",
        "path": "/hello",
        "queryStringParameters": None,
        "headers": {
            "Accept": "application/json",
            "Host": "localhost",
        },
        "body": None,
        "isBase64Encoded": False,
        "requestContext": {
            "resourceId": "abc123",
            "apiId": "xyz789",
            "resourcePath": "/hello",
            "httpMethod": "GET",
            "requestId": "test-request-id-002",
            "stage": "dev",
            "identity": {
                "sourceIp": "127.0.0.1",
                "userAgent": "pytest",
            },
        },
    }


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestLambdaHandlerWithCert:
    """Tests for requests that include a client certificate context."""

    def test_returns_200(self, apigw_event_with_cert: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        assert response["statusCode"] == 200

    def test_response_body_is_valid_json(
        self, apigw_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        body = json.loads(response["body"])
        assert isinstance(body, dict)

    def test_response_body_contains_message_key(
        self, apigw_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        body = json.loads(response["body"])
        assert "message" in body

    def test_response_body_message_is_non_empty(
        self, apigw_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        body = json.loads(response["body"])
        assert isinstance(body["message"], str)
        assert len(body["message"]) > 0

    def test_response_body_includes_client_subject(
        self, apigw_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        body = json.loads(response["body"])
        assert body.get("client_subject") == "CN=device-001,O=Supervictor"

    def test_response_has_json_content_type_header(
        self, apigw_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_with_cert, None)
        assert response.get("headers", {}).get("Content-Type") == "application/json"


class TestLambdaHandlerWithoutCert:
    """Tests for requests without a client certificate (dev/local use)."""

    def test_returns_200(self, apigw_event_no_cert: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_event_no_cert, None)
        assert response["statusCode"] == 200

    def test_response_body_omits_client_subject(
        self, apigw_event_no_cert: dict[str, Any]
    ) -> None:
        """client_subject must be absent (not null) when no cert is present."""
        response = app.lambda_handler(apigw_event_no_cert, None)
        body = json.loads(response["body"])
        assert "client_subject" not in body

    def test_response_body_contains_message_key(
        self, apigw_event_no_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_event_no_cert, None)
        body = json.loads(response["body"])
        assert "message" in body


class TestPostUplink:
    """Tests for POST /hello (device uplink)."""

    def test_returns_200(self, apigw_post_event: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_post_event, None)
        assert response["statusCode"] == 200

    def test_response_echoes_device_id(self, apigw_post_event: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_post_event, None)
        body = json.loads(response["body"])
        assert body["device_id"] == "1234567890"

    def test_response_echoes_current(self, apigw_post_event: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_post_event, None)
        body = json.loads(response["body"])
        assert body["current"] == 100

    def test_response_has_message(self, apigw_post_event: dict[str, Any]) -> None:
        response = app.lambda_handler(apigw_post_event, None)
        body = json.loads(response["body"])
        assert body["message"] == "Uplink received"

    def test_response_omits_client_subject_without_cert(
        self, apigw_post_event: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_post_event, None)
        body = json.loads(response["body"])
        assert "client_subject" not in body

    def test_response_includes_client_subject_with_cert(
        self, apigw_post_event_with_cert: dict[str, Any]
    ) -> None:
        response = app.lambda_handler(apigw_post_event_with_cert, None)
        body = json.loads(response["body"])
        assert body["client_subject"] == "CN=device-001,O=Supervictor"

    def test_missing_body_returns_400(self, apigw_post_event: dict[str, Any]) -> None:
        event = {**apigw_post_event, "body": None}
        response = app.lambda_handler(event, None)
        assert response["statusCode"] == 400

    def test_invalid_json_returns_422(self, apigw_post_event: dict[str, Any]) -> None:
        event = {**apigw_post_event, "body": '{"bad": "payload"}'}
        response = app.lambda_handler(event, None)
        assert response["statusCode"] == 422
