"""SQLite implementation of the DeviceStore protocol."""

import json
import os
import sqlite3

from uplink.store import DeviceRecord, UplinkRecord


class SqliteDeviceStore:
    """SQLite-backed device and uplink storage."""

    def __init__(self, db_path: str | None = None) -> None:
        self._db_path = db_path or os.environ.get("SQLITE_DB_PATH", ":memory:")
        self._conn = sqlite3.connect(self._db_path, check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._migrate()

    def _migrate(self) -> None:
        self._conn.executescript("""
            CREATE TABLE IF NOT EXISTS devices (
                device_id   TEXT PRIMARY KEY,
                owner_id    TEXT NOT NULL,
                subject_dn  TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                created_at  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS uplinks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id   TEXT NOT NULL,
                received_at TEXT NOT NULL,
                payload     TEXT NOT NULL
            );
        """)

    def put_device(self, record: DeviceRecord) -> DeviceRecord:
        try:
            self._conn.execute(
                "INSERT INTO devices (device_id, owner_id, subject_dn, status, created_at) "
                "VALUES (?, ?, ?, ?, ?)",
                (
                    record.device_id,
                    record.owner_id,
                    record.subject_dn,
                    record.status,
                    record.created_at,
                ),
            )
            self._conn.commit()
        except sqlite3.IntegrityError as exc:
            raise ValueError(f"Device already exists: {record.device_id}") from exc
        return record

    def get_device(self, device_id: str) -> DeviceRecord | None:
        row = self._conn.execute(
            "SELECT * FROM devices WHERE device_id = ?", (device_id,)
        ).fetchone()
        if row is None:
            return None
        return DeviceRecord(**dict(row))

    def list_devices(self) -> list[DeviceRecord]:
        rows = self._conn.execute("SELECT * FROM devices").fetchall()
        return [DeviceRecord(**dict(r)) for r in rows]

    def put_uplink(self, record: UplinkRecord) -> None:
        self._conn.execute(
            "INSERT INTO uplinks (device_id, received_at, payload) VALUES (?, ?, ?)",
            (record.device_id, record.received_at, json.dumps(record.payload)),
        )
        self._conn.commit()

    def get_uplinks(self, device_id: str, limit: int = 10) -> list[UplinkRecord]:
        rows = self._conn.execute(
            "SELECT * FROM uplinks WHERE device_id = ? ORDER BY received_at DESC LIMIT ?",
            (device_id, limit),
        ).fetchall()
        return [
            UplinkRecord(
                device_id=r["device_id"],
                received_at=r["received_at"],
                payload=json.loads(r["payload"]),
            )
            for r in rows
        ]
