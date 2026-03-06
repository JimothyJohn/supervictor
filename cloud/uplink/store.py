"""Device store protocol and data models."""

from typing import Protocol

from pydantic import BaseModel


class DeviceRecord(BaseModel):
    """A registered device."""

    device_id: str
    owner_id: str
    subject_dn: str | None = None
    status: str = "active"
    created_at: str  # ISO 8601


class UplinkRecord(BaseModel):
    """A single uplink message from a device."""

    device_id: str
    received_at: str  # ISO 8601
    payload: dict


class DeviceStore(Protocol):
    """Storage backend for devices and uplinks."""

    def put_device(self, record: DeviceRecord) -> DeviceRecord: ...
    def get_device(self, device_id: str) -> DeviceRecord | None: ...
    def list_devices(self) -> list[DeviceRecord]: ...
    def put_uplink(self, record: UplinkRecord) -> None: ...
    def get_uplinks(self, device_id: str, limit: int = 10) -> list[UplinkRecord]: ...
