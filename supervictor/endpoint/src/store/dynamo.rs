use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;

use crate::error::AppError;
use crate::models::{DeviceRecord, UplinkRecord};
use crate::store::DeviceStore;

/// DynamoDB-backed implementation of [`DeviceStore`](super::DeviceStore).
pub struct DynamoDeviceStore {
    client: Client,
    devices_table: String,
    messages_table: String,
}

/// Run an async block synchronously on the current tokio runtime.
/// Uses `block_in_place` to avoid panicking on multi-threaded runtimes.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| rt.block_on(f))
}

impl DynamoDeviceStore {
    /// Create a new DynamoDB store backed by the given AWS SDK config and table names.
    pub fn new(
        sdk_config: &aws_config::SdkConfig,
        devices_table: &str,
        messages_table: &str,
    ) -> Self {
        Self {
            client: Client::new(sdk_config),
            devices_table: devices_table.to_string(),
            messages_table: messages_table.to_string(),
        }
    }

    fn device_to_item(record: &DeviceRecord) -> std::collections::HashMap<String, AttributeValue> {
        let mut item = std::collections::HashMap::new();
        item.insert(
            "device_id".into(),
            AttributeValue::S(record.device_id.clone()),
        );
        item.insert(
            "owner_id".into(),
            AttributeValue::S(record.owner_id.clone()),
        );
        if let Some(ref dn) = record.subject_dn {
            item.insert("subject_dn".into(), AttributeValue::S(dn.clone()));
        }
        item.insert("status".into(), AttributeValue::S(record.status.clone()));
        item.insert(
            "created_at".into(),
            AttributeValue::S(record.created_at.clone()),
        );
        item
    }

    fn item_to_device(
        item: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<DeviceRecord, AppError> {
        let get_s = |key: &str| -> Result<String, AppError> {
            item.get(key)
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string())
                .ok_or_else(|| AppError::Store(format!("missing field: {key}")))
        };

        Ok(DeviceRecord {
            device_id: get_s("device_id")?,
            owner_id: get_s("owner_id")?,
            subject_dn: item
                .get("subject_dn")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string()),
            status: get_s("status")?,
            created_at: get_s("created_at")?,
        })
    }
}

impl DeviceStore for DynamoDeviceStore {
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError> {
        let item = Self::device_to_item(&record);
        let result = block_on(
            self.client
                .put_item()
                .table_name(&self.devices_table)
                .set_item(Some(item))
                .condition_expression("attribute_not_exists(device_id)")
                .send(),
        );

        match result {
            Ok(_) => Ok(record),
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_conditional_check_failed_exception() {
                    Err(AppError::DeviceAlreadyExists {
                        device_id: record.device_id,
                    })
                } else {
                    Err(AppError::Store(format!("put_device: {service_err}")))
                }
            }
        }
    }

    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError> {
        let result = block_on(
            self.client
                .get_item()
                .table_name(&self.devices_table)
                .key("device_id", AttributeValue::S(device_id.to_string()))
                .send(),
        )
        .map_err(|e| AppError::Store(format!("get_device: {e}")))?;

        match result.item {
            Some(ref item) => Ok(Some(Self::item_to_device(item)?)),
            None => Ok(None),
        }
    }

    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError> {
        let result = block_on(self.client.scan().table_name(&self.devices_table).send())
            .map_err(|e| AppError::Store(format!("list_devices: {e}")))?;

        result.items().iter().map(Self::item_to_device).collect()
    }

    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError> {
        let payload_str = serde_json::to_string(&record.payload)
            .map_err(|e| AppError::Store(format!("serialize payload: {e}")))?;

        block_on(
            self.client
                .put_item()
                .table_name(&self.messages_table)
                .item("device_id", AttributeValue::S(record.device_id))
                .item("received_at", AttributeValue::S(record.received_at))
                .item("payload", AttributeValue::S(payload_str))
                .send(),
        )
        .map_err(|e| AppError::Store(format!("put_uplink: {e}")))?;

        Ok(())
    }

    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError> {
        let result = block_on(
            self.client
                .query()
                .table_name(&self.messages_table)
                .key_condition_expression("device_id = :did")
                .expression_attribute_values(":did", AttributeValue::S(device_id.to_string()))
                .scan_index_forward(false)
                .limit(limit as i32)
                .send(),
        )
        .map_err(|e| AppError::Store(format!("get_uplinks: {e}")))?;

        result
            .items()
            .iter()
            .map(|item| {
                let device_id = item
                    .get("device_id")
                    .and_then(|v| v.as_s().ok())
                    .map(|s| s.to_string())
                    .ok_or_else(|| AppError::Store("missing device_id".into()))?;
                let received_at = item
                    .get("received_at")
                    .and_then(|v| v.as_s().ok())
                    .map(|s| s.to_string())
                    .ok_or_else(|| AppError::Store("missing received_at".into()))?;
                let payload_str = item
                    .get("payload")
                    .and_then(|v| v.as_s().ok())
                    .unwrap_or(&String::new())
                    .to_string();
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);

                Ok(UplinkRecord {
                    device_id,
                    received_at,
                    payload,
                })
            })
            .collect()
    }
}
