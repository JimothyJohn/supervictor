"""Pydantic models and OpenAPI spec for the Supervictor uplink API."""

from typing import Any

from pydantic import BaseModel


class UplinkMessage(BaseModel):
    """Incoming payload from Supervictor edge device."""

    id: str
    current: int


class HelloResponse(BaseModel):
    """Response payload for the root endpoint."""

    message: str
    client_subject: str | None = None


class UplinkResponse(BaseModel):
    """Response payload for POST / (uplink from device)."""

    message: str
    device_id: str
    current: int
    client_subject: str | None = None


class RegisterDeviceRequest(BaseModel):
    """Incoming payload for device registration."""

    device_id: str
    owner_id: str
    subject_dn: str | None = None


class DeviceResponse(BaseModel):
    """Response payload for device endpoints."""

    device_id: str
    owner_id: str
    subject_dn: str | None = None
    status: str
    created_at: str


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
            "/": {
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
