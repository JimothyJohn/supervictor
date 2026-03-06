"""Unit tests for device registration routes in the Starlette server."""

import pytest
import uplink.app as app_module
from starlette.testclient import TestClient
from uplink.store_sqlite import SqliteDeviceStore


@pytest.fixture(autouse=True)
def _fresh_store() -> None:
    """Replace the module-level store with a fresh in-memory SQLite store."""
    original = app_module.store
    app_module.store = SqliteDeviceStore(db_path=":memory:")
    yield
    app_module.store = original


client = TestClient(app_module.app)


def _register(device_id: str = "dev-001", owner_id: str = "owner-1") -> dict:
    return client.post(
        "/devices",
        json={"device_id": device_id, "owner_id": owner_id},
    ).json()


# ---------------------------------------------------------------------------
# POST /devices — register device
# ---------------------------------------------------------------------------


class TestPostDevices:
    def test_post_devices_201(self) -> None:
        resp = client.post(
            "/devices",
            json={
                "device_id": "dev-001",
                "owner_id": "owner-1",
                "subject_dn": "CN=dev-001",
            },
        )
        assert resp.status_code == 201
        body = resp.json()
        assert body["device_id"] == "dev-001"
        assert body["owner_id"] == "owner-1"
        assert body["subject_dn"] == "CN=dev-001"
        assert body["status"] == "active"
        assert "created_at" in body

    def test_post_devices_duplicate_409(self) -> None:
        client.post("/devices", json={"device_id": "dev-001", "owner_id": "o1"})
        resp = client.post("/devices", json={"device_id": "dev-001", "owner_id": "o1"})
        assert resp.status_code == 409
        assert resp.json()["error"] == "Device already exists"

    def test_post_devices_invalid_422(self) -> None:
        resp = client.post("/devices", json={"device_id": "dev-001"})
        assert resp.status_code == 422


# ---------------------------------------------------------------------------
# GET /devices — list devices
# ---------------------------------------------------------------------------


class TestGetDevices:
    def test_get_devices_empty_200(self) -> None:
        resp = client.get("/devices")
        assert resp.status_code == 200
        assert resp.json() == []

    def test_get_devices_with_data_200(self) -> None:
        _register("dev-001")
        _register("dev-002")
        resp = client.get("/devices")
        assert resp.status_code == 200
        assert len(resp.json()) == 2


# ---------------------------------------------------------------------------
# GET /devices/{device_id} — get single device
# ---------------------------------------------------------------------------


class TestGetDeviceById:
    def test_get_device_by_id_200(self) -> None:
        _register("dev-001")
        resp = client.get("/devices/dev-001")
        assert resp.status_code == 200
        assert resp.json()["device_id"] == "dev-001"

    def test_get_device_not_found_404(self) -> None:
        resp = client.get("/devices/nonexistent")
        assert resp.status_code == 404
        assert resp.json()["error"] == "Device not found"


# ---------------------------------------------------------------------------
# GET /devices/{device_id}/uplinks
# ---------------------------------------------------------------------------


class TestGetDeviceUplinks:
    def test_get_device_uplinks_empty_200(self) -> None:
        _register("dev-001")
        resp = client.get("/devices/dev-001/uplinks")
        assert resp.status_code == 200
        assert resp.json() == []


# ---------------------------------------------------------------------------
# Full roundtrip
# ---------------------------------------------------------------------------


class TestFullRoundtrip:
    def test_full_roundtrip(self) -> None:
        # Register device
        resp = client.post("/devices", json={"device_id": "dev-001", "owner_id": "owner-1"})
        assert resp.status_code == 201

        # Post uplink via existing POST / route
        resp = client.post("/", json={"id": "dev-001", "current": 99})
        assert resp.status_code == 200

        # Retrieve uplinks
        resp = client.get("/devices/dev-001/uplinks")
        assert resp.status_code == 200
        uplinks = resp.json()
        assert len(uplinks) == 1
        assert uplinks[0]["device_id"] == "dev-001"
        assert uplinks[0]["payload"] == {"current": 99}
