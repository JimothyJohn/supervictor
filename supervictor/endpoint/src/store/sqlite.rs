use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::error::AppError;
use crate::models::{DeviceRecord, UplinkRecord};
use crate::store::{DeviceStore, StoreError};

/// SQLite-backed implementation of [`DeviceStore`].
pub struct SqliteDeviceStore {
    conn: Mutex<Connection>,
}

impl SqliteDeviceStore {
    /// Open (or create) a SQLite database at `db_path` and run migrations.
    pub fn new(db_path: &str) -> Result<Self, AppError> {
        let conn = Connection::open(db_path)
            .map_err(|e| AppError::Store(StoreError::io("sqlite open", e)))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS devices (
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
            CREATE INDEX IF NOT EXISTS idx_uplinks_device_received
                ON uplinks(device_id, received_at DESC);",
        )
        .map_err(|e| AppError::Store(StoreError::io("migration", e)))?;
        Ok(())
    }
}

impl DeviceStore for SqliteDeviceStore {
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        conn.execute(
            "INSERT INTO devices (device_id, owner_id, subject_dn, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.device_id, record.owner_id, record.subject_dn, record.status, record.created_at],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                AppError::DeviceAlreadyExists {
                    device_id: record.device_id.clone(),
                }
            }
            other => AppError::Store(StoreError::io("put_device", other)),
        })?;
        Ok(record)
    }

    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        let mut stmt = conn
            .prepare("SELECT device_id, owner_id, subject_dn, status, created_at FROM devices WHERE device_id = ?1")
            .map_err(|e| AppError::Store(StoreError::io("get_device prepare", e)))?;

        let mut rows = stmt
            .query_map(params![device_id], |row| {
                Ok(DeviceRecord {
                    device_id: row.get(0)?,
                    owner_id: row.get(1)?,
                    subject_dn: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Store(StoreError::io("get_device query", e)))?;

        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(AppError::Store(StoreError::io("get_device row", e))),
            None => Ok(None),
        }
    }

    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        let mut stmt = conn
            .prepare("SELECT device_id, owner_id, subject_dn, status, created_at FROM devices")
            .map_err(|e| AppError::Store(StoreError::io("list_devices prepare", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(DeviceRecord {
                    device_id: row.get(0)?,
                    owner_id: row.get(1)?,
                    subject_dn: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Store(StoreError::io("list_devices query", e)))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Store(StoreError::io("list_devices collect", e)))
    }

    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        let payload_str = serde_json::to_string(&record.payload)
            .map_err(|e| AppError::Store(StoreError::serde("serialize payload", e)))?;
        conn.execute(
            "INSERT INTO uplinks (device_id, received_at, payload) VALUES (?1, ?2, ?3)",
            params![record.device_id, record.received_at, payload_str],
        )
        .map_err(|e| AppError::Store(StoreError::io("put_uplink", e)))?;
        Ok(())
    }

    fn set_device_status(&self, device_id: &str, status: &str) -> Result<DeviceRecord, AppError> {
        {
            let conn = self
                .conn
                .lock()
                .map_err(|_| AppError::Store(StoreError::Poisoned))?;
            let updated = conn
                .execute(
                    "UPDATE devices SET status = ?2 WHERE device_id = ?1",
                    params![device_id, status],
                )
                .map_err(|e| AppError::Store(StoreError::io("set_device_status", e)))?;
            if updated == 0 {
                return Err(AppError::DeviceNotFound {
                    device_id: device_id.to_string(),
                });
            }
        }
        self.get_device(device_id)?.ok_or(AppError::DeviceNotFound {
            device_id: device_id.to_string(),
        })
    }

    fn latest_uplinks(&self) -> Result<Vec<UplinkRecord>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        // MAX(received_at) with bare columns: SQLite guarantees the other
        // selected columns come from the max row (documented behavior).
        let mut stmt = conn
            .prepare("SELECT device_id, MAX(received_at), payload FROM uplinks GROUP BY device_id")
            .map_err(|e| AppError::Store(StoreError::io("latest_uplinks prepare", e)))?;

        let rows = stmt
            .query_map([], |row| {
                let payload_str: String = row.get(2)?;
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
                Ok(UplinkRecord {
                    device_id: row.get(0)?,
                    received_at: row.get(1)?,
                    payload,
                })
            })
            .map_err(|e| AppError::Store(StoreError::io("latest_uplinks query", e)))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Store(StoreError::io("latest_uplinks collect", e)))
    }

    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Store(StoreError::Poisoned))?;
        let mut stmt = conn
            .prepare("SELECT device_id, received_at, payload FROM uplinks WHERE device_id = ?1 ORDER BY received_at DESC LIMIT ?2")
            .map_err(|e| AppError::Store(StoreError::io("get_uplinks prepare", e)))?;

        // rusqlite 0.40 dropped ToSql for usize (platform-width); bind i64.
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(params![device_id, limit], |row| {
                let payload_str: String = row.get(2)?;
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
                Ok(UplinkRecord {
                    device_id: row.get(0)?,
                    received_at: row.get(1)?,
                    payload,
                })
            })
            .map_err(|e| AppError::Store(StoreError::io("get_uplinks query", e)))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Store(StoreError::io("get_uplinks collect", e)))
    }
}
