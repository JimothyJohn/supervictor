"""Supervictor hello-world Lambda handler.

Provides a simple health/greeting endpoint for the Supervictor edge device.
mTLS enforcement is handled at the API Gateway custom domain level; this handler
extracts the client certificate subject DN for logging and response enrichment.
"""

import json
import logging
from typing import Any

from pydantic import BaseModel, ValidationError

logger = logging.getLogger(__name__)
logger.setLevel(logging.INFO)


# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------


class UplinkMessage(BaseModel):
    """Incoming payload from Supervictor edge device."""

    id: str
    current: int


class HelloResponse(BaseModel):
    """Response payload for the /hello endpoint."""

    message: str
    client_subject: str | None = None


class UplinkResponse(BaseModel):
    """Response payload for POST /hello (uplink from device)."""

    message: str
    device_id: str
    current: int
    client_subject: str | None = None


# ---------------------------------------------------------------------------
# OpenAPI
# ---------------------------------------------------------------------------


def openapi_spec() -> dict[str, Any]:
    """Generate an OpenAPI 3.1.0 spec from the Pydantic models."""
    return {
        "openapi": "3.1.0",
        "info": {
            "title": "Supervictor API",
            "version": "0.1.0",
            "description": "Companion API for Supervictor edge device.",
        },
        "paths": {
            "/hello": {
                "get": {
                    "summary": "Health check / greeting",
                    "operationId": "getHello",
                    "responses": {
                        "200": {
                            "description": "Greeting response",
                            "content": {
                                "application/json": {
                                    "schema": HelloResponse.model_json_schema(),
                                }
                            },
                        }
                    },
                },
                "post": {
                    "summary": "Device uplink",
                    "operationId": "postUplink",
                    "requestBody": {
                        "required": True,
                        "content": {
                            "application/json": {
                                "schema": UplinkMessage.model_json_schema(),
                            }
                        },
                    },
                    "responses": {
                        "200": {
                            "description": "Uplink accepted",
                            "content": {
                                "application/json": {
                                    "schema": UplinkResponse.model_json_schema(),
                                }
                            },
                        },
                        "400": {
                            "description": "Missing request body",
                        },
                        "422": {
                            "description": "Invalid payload",
                        },
                    },
                },
            }
        },
    }


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _extract_client_subject(event: dict[str, Any]) -> str | None:
    """Extract the mTLS client certificate subject DN from the API Gateway event.

    Args:
        event: API Gateway Lambda proxy input event.

    Returns:
        The subjectDN string when a client certificate is present, else None.
    """
    try:
        return event["requestContext"]["identity"]["clientCert"]["subjectDN"]
    except (KeyError, TypeError):
        return None


# ---------------------------------------------------------------------------
# Handler
# ---------------------------------------------------------------------------


def lambda_handler(event: dict[str, Any], context: Any) -> dict[str, Any]:
    """Handle incoming API Gateway requests on /hello.

    GET  — health/greeting response.
    POST — accept UplinkMessage JSON from edge device.

    Args:
        event: API Gateway Lambda proxy input event.
        context: Lambda context runtime object.

    Returns:
        API Gateway Lambda proxy response dict with statusCode, headers, and body.
    """
    client_subject = _extract_client_subject(event)
    http_method = event.get("httpMethod", "GET")

    logger.info(
        "Request received",
        extra={
            "path": event.get("path"),
            "method": http_method,
            "client_subject": client_subject,
            "request_id": event.get("requestContext", {}).get("requestId"),
        },
    )

    if http_method == "POST":
        return _handle_post(event, client_subject)

    response = HelloResponse(
        message="Hello from Supervictor!",
        client_subject=client_subject,
    )

    return {
        "statusCode": 200,
        "headers": {"Content-Type": "application/json"},
        "body": response.model_dump_json(exclude_none=True),
    }


def _handle_post(
    event: dict[str, Any], client_subject: str | None
) -> dict[str, Any]:
    """Handle POST /hello — parse UplinkMessage from device."""
    raw_body = event.get("body")
    if not raw_body:
        return {
            "statusCode": 400,
            "headers": {"Content-Type": "application/json"},
            "body": json.dumps({"error": "Missing request body"}),
        }

    try:
        parsed = json.loads(raw_body) if isinstance(raw_body, str) else raw_body
        uplink = UplinkMessage.model_validate(parsed)
    except (json.JSONDecodeError, ValidationError) as exc:
        logger.warning("Invalid uplink payload", extra={"errors": exc.errors()})
        return {
            "statusCode": 422,
            "headers": {"Content-Type": "application/json"},
            "body": json.dumps({"error": "Invalid payload", "detail": exc.errors()}),
        }

    logger.info(
        "Uplink received",
        extra={"device_id": uplink.id, "current": uplink.current},
    )

    response = UplinkResponse(
        message="Uplink received",
        device_id=uplink.id,
        current=uplink.current,
        client_subject=client_subject,
    )

    return {
        "statusCode": 200,
        "headers": {"Content-Type": "application/json"},
        "body": response.model_dump_json(exclude_none=True),
    }
