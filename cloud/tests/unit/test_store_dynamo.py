"""Unit tests for the DynamoDB device store using moto."""

import boto3
import pytest
from moto import mock_aws
from uplink.store import DeviceRecord, UplinkRecord
from uplink.store_dynamo import DynamoDeviceStore


def _create_tables() -> None:
    """Create the DynamoDB tables expected by DynamoDeviceStore."""
    dynamodb = boto3.resource("dynamodb", region_name="us-east-1")
    dynamodb.create_table(
        TableName="devices",
        KeySchema=[{"AttributeName": "device_id", "KeyType": "HASH"}],
        AttributeDefinitions=[{"AttributeName": "device_id", "AttributeType": "S"}],
        BillingMode="PAY_PER_REQUEST",
    )
    dynamodb.create_table(
        TableName="messages",
        KeySchema=[
            {"AttributeName": "device_id", "KeyType": "HASH"},
            {"AttributeName": "received_at", "KeyType": "RANGE"},
        ],
        AttributeDefinitions=[
            {"AttributeName": "device_id", "AttributeType": "S"},
            {"AttributeName": "received_at", "AttributeType": "S"},
        ],
        BillingMode="PAY_PER_REQUEST",
    )


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


@pytest.fixture(autouse=True)
def _aws_env(monkeypatch: pytest.MonkeyPatch) -> None:
    """Ensure boto3 uses moto's fake credentials."""
    monkeypatch.setenv("AWS_ACCESS_KEY_ID", "testing")
    monkeypatch.setenv("AWS_SECRET_ACCESS_KEY", "testing")
    monkeypatch.setenv("AWS_SECURITY_TOKEN", "testing")
    monkeypatch.setenv("AWS_SESSION_TOKEN", "testing")
    monkeypatch.setenv("AWS_DEFAULT_REGION", "us-east-1")
    monkeypatch.setenv("DEVICES_TABLE", "devices")
    monkeypatch.setenv("MESSAGES_TABLE", "messages")


class TestDynamoPutAndGetDevice:
    @mock_aws
    def test_put_and_get_device(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        record = _device()
        store.put_device(record)
        got = store.get_device("dev-001")
        assert got is not None
        assert got.device_id == "dev-001"
        assert got.owner_id == "owner-1"
        assert got.status == "active"
        assert got.created_at == "2026-03-01T00:00:00Z"

    @mock_aws
    def test_get_device_not_found(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        assert store.get_device("nonexistent") is None

    @mock_aws
    def test_put_device_duplicate_raises(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        store.put_device(_device())
        with pytest.raises(ValueError, match="Device already exists"):
            store.put_device(_device())


class TestDynamoListDevices:
    @mock_aws
    def test_list_devices_empty(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        assert store.list_devices() == []

    @mock_aws
    def test_list_devices_multiple(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        store.put_device(_device("dev-001"))
        store.put_device(_device("dev-002"))
        store.put_device(_device("dev-003"))
        devices = store.list_devices()
        assert len(devices) == 3
        ids = {d.device_id for d in devices}
        assert ids == {"dev-001", "dev-002", "dev-003"}


class TestDynamoPutAndGetUplinks:
    @mock_aws
    def test_put_and_get_uplinks(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        store.put_uplink(_uplink())
        uplinks = store.get_uplinks("dev-001")
        assert len(uplinks) == 1
        assert uplinks[0].device_id == "dev-001"
        assert uplinks[0].payload == {"temperature": 22.5}

    @mock_aws
    def test_get_uplinks_empty(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        assert store.get_uplinks("dev-001") == []

    @mock_aws
    def test_get_uplinks_respects_limit(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
        for i in range(5):
            store.put_uplink(_uplink(received_at=f"2026-03-01T00:00:0{i}Z"))
        uplinks = store.get_uplinks("dev-001", limit=3)
        assert len(uplinks) == 3

    @mock_aws
    def test_get_uplinks_ordered_desc(self) -> None:
        _create_tables()
        store = DynamoDeviceStore()
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
