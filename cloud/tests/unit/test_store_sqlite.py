"""Unit tests for the SQLite device store."""

from pathlib import Path

import pytest
from uplink.store import DeviceRecord, UplinkRecord
from uplink.store_sqlite import SqliteDeviceStore


def _make_store(tmp_path: Path) -> SqliteDeviceStore:
    return SqliteDeviceStore(db_path=str(tmp_path / "test.db"))


def _device(device_id: str = "dev-001", owner_id: str = "owner-1") -> DeviceRecord:
    return DeviceRecord(
        device_id=device_id,
        owner_id=owner_id,
        created_at="2026-03-01T00:00:00Z",
    )


def _uplink(
    device_id: str = "dev-001",
    received_at: str = "2026-03-01T00:00:00Z",
    payload: dict | None = None,
) -> UplinkRecord:
    return UplinkRecord(
        device_id=device_id,
        received_at=received_at,
        payload=payload or {"temperature": 22.5},
    )


# ---------------------------------------------------------------------------
# Devices
# ---------------------------------------------------------------------------


class TestPutAndGetDevice:
    def test_put_and_get_device(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        record = _device()
        store.put_device(record)
        got = store.get_device("dev-001")
        assert got is not None
        assert got.device_id == "dev-001"
        assert got.owner_id == "owner-1"
        assert got.status == "active"
        assert got.created_at == "2026-03-01T00:00:00Z"

    def test_get_device_not_found(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        assert store.get_device("nonexistent") is None

    def test_put_device_duplicate(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        store.put_device(_device())
        with pytest.raises(ValueError, match="Device already exists"):
            store.put_device(_device())


class TestListDevices:
    def test_list_devices_empty(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        assert store.list_devices() == []

    def test_list_devices_one(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        store.put_device(_device())
        devices = store.list_devices()
        assert len(devices) == 1
        assert devices[0].device_id == "dev-001"

    def test_list_devices_many(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        store.put_device(_device("dev-001"))
        store.put_device(_device("dev-002"))
        store.put_device(_device("dev-003"))
        devices = store.list_devices()
        assert len(devices) == 3


# ---------------------------------------------------------------------------
# Uplinks
# ---------------------------------------------------------------------------


class TestPutAndGetUplinks:
    def test_put_and_get_uplinks(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        store.put_uplink(_uplink())
        uplinks = store.get_uplinks("dev-001")
        assert len(uplinks) == 1
        assert uplinks[0].device_id == "dev-001"
        assert uplinks[0].payload == {"temperature": 22.5}

    def test_get_uplinks_empty(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        assert store.get_uplinks("dev-001") == []

    def test_get_uplinks_limit(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        for i in range(5):
            store.put_uplink(_uplink(received_at=f"2026-03-01T00:00:0{i}Z"))
        uplinks = store.get_uplinks("dev-001", limit=3)
        assert len(uplinks) == 3

    def test_get_uplinks_ordered_by_received_at_desc(self, tmp_path: Path) -> None:
        store = _make_store(tmp_path)
        store.put_uplink(_uplink(received_at="2026-03-01T00:00:01Z"))
        store.put_uplink(_uplink(received_at="2026-03-01T00:00:03Z"))
        store.put_uplink(_uplink(received_at="2026-03-01T00:00:02Z"))
        uplinks = store.get_uplinks("dev-001")
        timestamps = [u.received_at for u in uplinks]
        assert timestamps == [
            "2026-03-01T00:00:03Z",
            "2026-03-01T00:00:02Z",
            "2026-03-01T00:00:01Z",
        ]
