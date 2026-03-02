"""Pure business logic for the Supervictor uplink API.

Framework-agnostic: takes plain Python args, returns Pydantic models or dicts.
No Lambda event dicts, no HTTP framework objects.
"""

import json
import logging

from pydantic import ValidationError

from uplink.models import HelloResponse, UplinkMessage, UplinkResponse

logger = logging.getLogger(__name__)


def handle_hello(
    *, client_subject: str | None = None
) -> HelloResponse:
    """Handle GET / — health check / greeting."""
    return HelloResponse(
        message="Hello from Supervictor!",
        client_subject=client_subject,
    )


def handle_uplink(
    raw_body: str | None,
    *,
    client_subject: str | None = None,
) -> tuple[UplinkResponse | dict, int]:
    """Handle POST / — parse and validate an uplink message from a device.

    Returns:
        (response, status_code) where response is a Pydantic model (success)
        or a dict (error).
    """
    if not (raw_body or "").strip():
        return {"error": "Missing request body"}, 400

    try:
        parsed = json.loads(raw_body) if isinstance(raw_body, str) else raw_body
    except json.JSONDecodeError as exc:
        logger.warning("Malformed JSON in uplink payload", extra={"error": str(exc)})
        return {"error": "Invalid payload", "detail": str(exc)}, 422

    try:
        uplink = UplinkMessage.model_validate(parsed)
    except ValidationError as exc:
        logger.warning("Invalid uplink payload", extra={"errors": exc.errors()})
        return {"error": "Invalid payload", "detail": exc.errors()}, 422

    logger.info(
        "Uplink received",
        extra={"device_id": uplink.id, "current": uplink.current},
    )

    return UplinkResponse(
        message="Uplink received",
        device_id=uplink.id,
        current=uplink.current,
        client_subject=client_subject,
    ), 200
