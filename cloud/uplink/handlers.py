"""Pure business logic for the Supervictor uplink API.

Framework-agnostic: takes plain Python args, returns Pydantic models or dicts.
No Lambda event dicts, no HTTP framework objects.
"""

import json
import logging
from datetime import UTC, datetime

from pydantic import ValidationError
from uplink.models import (
    DeviceResponse,
    HelloResponse,
    RegisterDeviceRequest,
    UplinkMessage,
    UplinkResponse,
)
from uplink.store import DeviceRecord, DeviceStore, UplinkRecord

logger = logging.getLogger(__name__)


def handle_hello(*, client_subject: str | None = None) -> HelloResponse:
    """Handle GET / — health check / greeting."""
    return HelloResponse(
        message="Hello from Supervictor!",
        client_subject=client_subject,
    )


def handle_uplink(
    raw_body: str | None,
    *,
    client_subject: str | None = None,
    store: DeviceStore | None = None,
    require_registration: bool = False,
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

    if require_registration and store is not None:
        device = store.get_device(uplink.id)
        if device is None or device.status != "active":
            return {"error": "Device not registered or inactive"}, 403

    if store is not None:
        store.put_uplink(
            UplinkRecord(
                device_id=uplink.id,
                received_at=datetime.now(UTC).isoformat(),
                payload={"current": uplink.current},
            )
        )

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


def handle_register_device(
    raw_body: str | None,
    *,
    store: DeviceStore,
) -> tuple[DeviceResponse | dict, int]:
    """Handle POST /devices — register a new device."""
    if not (raw_body or "").strip():
        return {"error": "Missing request body"}, 400

    try:
        parsed = json.loads(raw_body) if isinstance(raw_body, str) else raw_body
    except json.JSONDecodeError as exc:
        return {"error": "Invalid payload", "detail": str(exc)}, 422

    try:
        req = RegisterDeviceRequest.model_validate(parsed)
    except ValidationError as exc:
        return {"error": "Invalid payload", "detail": exc.errors()}, 422

    record = DeviceRecord(
        device_id=req.device_id,
        owner_id=req.owner_id,
        subject_dn=req.subject_dn,
        status="active",
        created_at=datetime.now(UTC).isoformat(),
    )

    try:
        store.put_device(record)
    except ValueError:
        return {"error": "Device already exists"}, 409

    return DeviceResponse(
        device_id=record.device_id,
        owner_id=record.owner_id,
        subject_dn=record.subject_dn,
        status=record.status,
        created_at=record.created_at,
    ), 201


def handle_get_device(
    device_id: str,
    *,
    store: DeviceStore,
) -> tuple[DeviceResponse | dict, int]:
    """Handle GET /devices/{device_id} — look up a single device."""
    device = store.get_device(device_id)
    if device is None:
        return {"error": "Device not found"}, 404
    return DeviceResponse(
        device_id=device.device_id,
        owner_id=device.owner_id,
        subject_dn=device.subject_dn,
        status=device.status,
        created_at=device.created_at,
    ), 200


def handle_list_devices(
    *,
    store: DeviceStore,
) -> tuple[list[DeviceResponse], int]:
    """Handle GET /devices — list all devices."""
    devices = store.list_devices()
    return [
        DeviceResponse(
            device_id=d.device_id,
            owner_id=d.owner_id,
            subject_dn=d.subject_dn,
            status=d.status,
            created_at=d.created_at,
        )
        for d in devices
    ], 200


def handle_get_device_uplinks(
    device_id: str,
    *,
    store: DeviceStore,
    limit: int = 10,
) -> tuple[list[dict], int]:
    """Handle GET /devices/{device_id}/uplinks — list recent uplinks."""
    uplinks = store.get_uplinks(device_id, limit=limit)
    return [u.model_dump() for u in uplinks], 200
