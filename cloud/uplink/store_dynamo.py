"""DynamoDB implementation of the DeviceStore protocol."""

import json
import os

from uplink.store import DeviceRecord, UplinkRecord


class DynamoDeviceStore:
    """DynamoDB-backed device and uplink storage."""

    def __init__(self) -> None:
        import boto3

        self._devices_table_name = os.environ.get("DEVICES_TABLE", "devices")
        self._messages_table_name = os.environ.get("MESSAGES_TABLE", "messages")
        dynamodb = boto3.resource("dynamodb")
        self._devices_table = dynamodb.Table(self._devices_table_name)
        self._messages_table = dynamodb.Table(self._messages_table_name)

    def put_device(self, record: DeviceRecord) -> DeviceRecord:
        try:
            self._devices_table.put_item(
                Item=record.model_dump(),
                ConditionExpression="attribute_not_exists(device_id)",
            )
        except self._devices_table.meta.client.exceptions.ConditionalCheckFailedException as exc:
            raise ValueError(f"Device already exists: {record.device_id}") from exc
        return record

    def get_device(self, device_id: str) -> DeviceRecord | None:
        resp = self._devices_table.get_item(Key={"device_id": device_id})
        item = resp.get("Item")
        if item is None:
            return None
        return DeviceRecord(**item)

    def list_devices(self) -> list[DeviceRecord]:
        resp = self._devices_table.scan()
        return [DeviceRecord(**item) for item in resp.get("Items", [])]

    def put_uplink(self, record: UplinkRecord) -> None:
        self._messages_table.put_item(
            Item={
                "device_id": record.device_id,
                "received_at": record.received_at,
                "payload": json.dumps(record.payload),
            }
        )

    def get_uplinks(self, device_id: str, limit: int = 10) -> list[UplinkRecord]:
        resp = self._messages_table.query(
            KeyConditionExpression="device_id = :did",
            ExpressionAttributeValues={":did": device_id},
            ScanIndexForward=False,
            Limit=limit,
        )
        return [
            UplinkRecord(
                device_id=item["device_id"],
                received_at=item["received_at"],
                payload=json.loads(item["payload"]),
            )
            for item in resp.get("Items", [])
        ]
