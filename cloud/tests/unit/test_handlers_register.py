"""Unit tests for device registration and store-aware uplink handlers.

Uses a real SQLite store (in-memory) — no mocks.
"""

import json

import pytest
from uplink.handlers import (
    handle_get_device,
    handle_get_device_uplinks,
    handle_list_devices,
    handle_register_device,
    handle_uplink,
)
from uplink.models import DeviceResponse
from uplink.store_sqlite import SqliteDeviceStore


@pytest.fixture
def store() -> SqliteDeviceStore:
    return SqliteDeviceStore(db_path=":memory:")


def _register(store: SqliteDeviceStore, device_id: str = "dev-001") -> None:
    """Helper to register a device via the handler."""
    body = json.dumps({"device_id": device_id, "owner_id": "owner-1"})
    handle_register_device(body, store=store)


# ---------------------------------------------------------------------------
# POST /devices — handle_register_device
# ---------------------------------------------------------------------------


class TestRegisterDevice:
    def test_register_device_success(self, store: SqliteDeviceStore) -> None:
        body = json.dumps(
            {
                "device_id": "dev-001",
                "owner_id": "owner-1",
                "subject_dn": "CN=dev-001",
            }
        )
        result, status = handle_register_device(body, store=store)
        assert status == 201
        assert isinstance(result, DeviceResponse)
        assert result.device_id == "dev-001"
        assert result.owner_id == "owner-1"
        assert result.subject_dn == "CN=dev-001"
        assert result.status == "active"
        assert result.created_at  # non-empty ISO string

    def test_register_device_duplicate(self, store: SqliteDeviceStore) -> None:
        body = json.dumps({"device_id": "dev-001", "owner_id": "owner-1"})
        handle_register_device(body, store=store)
        result, status = handle_register_device(body, store=store)
        assert status == 409
        assert result["error"] == "Device already exists"

    def test_register_device_missing_fields(self, store: SqliteDeviceStore) -> None:
        body = json.dumps({"device_id": "dev-001"})
        result, status = handle_register_device(body, store=store)
        assert status == 422
        assert result["error"] == "Invalid payload"

    def test_register_device_empty_body(self, store: SqliteDeviceStore) -> None:
        result, status = handle_register_device("", store=store)
        assert status == 400
        assert result["error"] == "Missing request body"


# ---------------------------------------------------------------------------
# GET /devices/{id} — handle_get_device
# ---------------------------------------------------------------------------


class TestGetDevice:
    def test_get_device_found(self, store: SqliteDeviceStore) -> None:
        _register(store)
        result, status = handle_get_device("dev-001", store=store)
        assert status == 200
        assert isinstance(result, DeviceResponse)
        assert result.device_id == "dev-001"

    def test_get_device_not_found(self, store: SqliteDeviceStore) -> None:
        result, status = handle_get_device("nonexistent", store=store)
        assert status == 404
        assert result["error"] == "Device not found"


# ---------------------------------------------------------------------------
# GET /devices — handle_list_devices
# ---------------------------------------------------------------------------


class TestListDevices:
    def test_list_devices_empty(self, store: SqliteDeviceStore) -> None:
        result, status = handle_list_devices(store=store)
        assert status == 200
        assert result == []

    def test_list_devices_with_data(self, store: SqliteDeviceStore) -> None:
        _register(store, "dev-001")
        _register(store, "dev-002")
        result, status = handle_list_devices(store=store)
        assert status == 200
        assert len(result) == 2


# ---------------------------------------------------------------------------
# GET /devices/{id}/uplinks — handle_get_device_uplinks
# ---------------------------------------------------------------------------


class TestGetDeviceUplinks:
    def test_get_device_uplinks(self, store: SqliteDeviceStore) -> None:
        _register(store)
        # Send an uplink so there's data
        handle_uplink(
            '{"id":"dev-001","current":42}',
            store=store,
        )
        result, status = handle_get_device_uplinks("dev-001", store=store)
        assert status == 200
        assert len(result) == 1
        assert result[0]["device_id"] == "dev-001"
        assert result[0]["payload"] == {"current": 42}


# ---------------------------------------------------------------------------
# POST / with store — handle_uplink registration checks
# ---------------------------------------------------------------------------


class TestUplinkWithStore:
    def test_uplink_with_registration_check_active(self, store: SqliteDeviceStore) -> None:
        _register(store)
        result, status = handle_uplink(
            '{"id":"dev-001","current":100}',
            store=store,
            require_registration=True,
        )
        assert status == 200
        # Verify the uplink was recorded
        uplinks = store.get_uplinks("dev-001")
        assert len(uplinks) == 1

    def test_uplink_with_registration_check_not_found(self, store: SqliteDeviceStore) -> None:
        result, status = handle_uplink(
            '{"id":"unknown-device","current":100}',
            store=store,
            require_registration=True,
        )
        assert status == 403
        assert result["error"] == "Device not registered or inactive"

    def test_uplink_without_store_backward_compat(self) -> None:
        result, status = handle_uplink('{"id":"1234567890","current":100}')
        assert status == 200
        assert result.device_id == "1234567890"
        assert result.current == 100
