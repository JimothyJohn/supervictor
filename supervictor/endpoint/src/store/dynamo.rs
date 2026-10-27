use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;

use crate::error::AppError;
use crate::models::{DeviceRecord, DeviceStyle, UplinkRecord};
use crate::store::{DeviceStore, StoreError};
use supervictor_common::models::{LineStyle, Marker};

/// DynamoDB-backed implementation of [`DeviceStore`](super::DeviceStore).
pub struct DynamoDeviceStore {
    client: Client,
    devices_table: String,
    messages_table: String,
    /// Days before an uplink row expires via DynamoDB TTL; 0 disables.
    uplink_ttl_days: u32,
}

/// Run an async block synchronously on the current tokio runtime.
/// Uses `block_in_place` to avoid panicking on multi-threaded runtimes.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| rt.block_on(f))
}

impl DynamoDeviceStore {
    /// Create a new DynamoDB store backed by the given AWS SDK config and
    /// table names. `uplink_ttl_days` stamps each uplink row with an
    /// `expires_at` epoch attribute for DynamoDB TTL (0 = rows never expire);
    /// the table's TimeToLiveSpecification (template.yaml) does the reaping.
    pub fn new(
        sdk_config: &aws_config::SdkConfig,
        devices_table: &str,
        messages_table: &str,
        uplink_ttl_days: u32,
    ) -> Self {
        Self {
            client: Client::new(sdk_config),
            devices_table: devices_table.to_string(),
            messages_table: messages_table.to_string(),
            uplink_ttl_days,
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
        if let Some(t) = record.power_threshold_ma {
            item.insert(
                "power_threshold_ma".into(),
                AttributeValue::N(t.to_string()),
            );
        }
        if let Some(m) = record.min_on_s {
            item.insert("min_on_s".into(), AttributeValue::N(m.to_string()));
        }
        for (key, value) in Self::style_attributes(&record.style) {
            if let Some(value) = value {
                item.insert(key.into(), value);
            }
        }
        if let Some(ref fw) = record.desired_fw {
            item.insert("desired_fw".into(), AttributeValue::S(fw.clone()));
        }
        item
    }

    /// The three style attributes as (name, value-or-absent), shared by
    /// `put_device` and `set_device_style`.
    fn style_attributes(style: &DeviceStyle) -> [(&'static str, Option<AttributeValue>); 3] {
        [
            ("color", style.color.clone().map(AttributeValue::S)),
            (
                "line_style",
                style
                    .line_style
                    .map(|l| AttributeValue::S(l.label().to_string())),
            ),
            (
                "marker",
                style
                    .marker
                    .map(|m| AttributeValue::S(m.label().to_string())),
            ),
        ]
    }

    fn item_to_device(
        item: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<DeviceRecord, AppError> {
        let get_s = |key: &str| -> Result<String, AppError> {
            item.get(key)
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    AppError::Store(StoreError::io(
                        "item_to_device",
                        format!("missing field: {key}"),
                    ))
                })
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
            // A stored but unparseable number is a corrupt row, not "no
            // override": surface it rather than silently judging against
            // the fleet default.
            power_threshold_ma: match item.get("power_threshold_ma") {
                None => None,
                Some(v) => Some(
                    v.as_n()
                        .ok()
                        .and_then(|n| n.parse::<i64>().ok())
                        .ok_or_else(|| {
                            AppError::Store(StoreError::io(
                                "item_to_device",
                                "power_threshold_ma is not an integer",
                            ))
                        })?,
                ),
            },
            min_on_s: match item.get("min_on_s") {
                None => None,
                Some(v) => Some(
                    v.as_n()
                        .ok()
                        .and_then(|n| n.parse::<u32>().ok())
                        .ok_or_else(|| {
                            AppError::Store(StoreError::io(
                                "item_to_device",
                                "min_on_s is not a u32",
                            ))
                        })?,
                )
                .filter(|m| *m > 0),
            },
            // Unknown style tokens read as unset (a newer binary may have
            // written them); the colour is validated where it is drawn.
            style: DeviceStyle {
                color: item
                    .get("color")
                    .and_then(|v| v.as_s().ok())
                    .map(|s| s.to_string()),
                line_style: item
                    .get("line_style")
                    .and_then(|v| v.as_s().ok())
                    .and_then(|s| LineStyle::parse(s)),
                marker: item
                    .get("marker")
                    .and_then(|v| v.as_s().ok())
                    .and_then(|s| Marker::parse(s)),
            },
            desired_fw: item
                .get("desired_fw")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string()),
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
                    Err(AppError::Store(StoreError::io("put_device", service_err)))
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
        .map_err(|e| AppError::Store(StoreError::io("get_device", e)))?;

        match result.item {
            Some(ref item) => Ok(Some(Self::item_to_device(item)?)),
            None => Ok(None),
        }
    }

    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError> {
        let result = block_on(self.client.scan().table_name(&self.devices_table).send())
            .map_err(|e| AppError::Store(StoreError::io("list_devices", e)))?;

        result.items().iter().map(Self::item_to_device).collect()
    }

    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError> {
        let payload_str = serde_json::to_string(&record.payload)
            .map_err(|e| AppError::Store(StoreError::serde("serialize payload", e)))?;

        let expires_at =
            crate::store::uplink_expires_at_unix(&record.received_at, self.uplink_ttl_days);

        let mut put = self
            .client
            .put_item()
            .table_name(&self.messages_table)
            .item("device_id", AttributeValue::S(record.device_id))
            .item("received_at", AttributeValue::S(record.received_at))
            .item("payload", AttributeValue::S(payload_str));
        if let Some(epoch) = expires_at {
            put = put.item("expires_at", AttributeValue::N(epoch.to_string()));
        }

        block_on(put.send()).map_err(|e| AppError::Store(StoreError::io("put_uplink", e)))?;

        Ok(())
    }

    fn set_device_status(&self, device_id: &str, status: &str) -> Result<DeviceRecord, AppError> {
        let result = block_on(
            self.client
                .update_item()
                .table_name(&self.devices_table)
                .key("device_id", AttributeValue::S(device_id.to_string()))
                .update_expression("SET #s = :status")
                .expression_attribute_names("#s", "status")
                .expression_attribute_values(":status", AttributeValue::S(status.to_string()))
                .condition_expression("attribute_exists(device_id)")
                .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
                .send(),
        );

        match result {
            Ok(out) => {
                let item = out.attributes().ok_or_else(|| {
                    AppError::Store(StoreError::io(
                        "set_device_status",
                        "no attributes returned",
                    ))
                })?;
                Self::item_to_device(item)
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_conditional_check_failed_exception() {
                    Err(AppError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                } else {
                    Err(AppError::Store(StoreError::io(
                        "set_device_status",
                        service_err,
                    )))
                }
            }
        }
    }

    /// One query per device (`limit 1`, newest first). Fine at current fleet
    /// sizes; at 100+ devices pre-aggregate a latest-uplink item instead
    /// (TODO.md / FRONTEND plan open question).
    fn set_device_threshold(
        &self,
        device_id: &str,
        threshold_ma: Option<i64>,
        min_on_s: Option<u32>,
    ) -> Result<DeviceRecord, AppError> {
        // One update expression setting the present parts and removing
        // the absent ones: `SET #t = :t REMOVE #m` and every other combo.
        let mut request = self
            .client
            .update_item()
            .table_name(&self.devices_table)
            .key("device_id", AttributeValue::S(device_id.to_string()))
            .expression_attribute_names("#t", "power_threshold_ma")
            .expression_attribute_names("#m", "min_on_s")
            .condition_expression("attribute_exists(device_id)")
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew);
        let mut sets: Vec<&str> = Vec::new();
        let mut removes: Vec<&str> = Vec::new();
        match threshold_ma {
            Some(t) => {
                sets.push("#t = :t");
                request =
                    request.expression_attribute_values(":t", AttributeValue::N(t.to_string()));
            }
            None => removes.push("#t"),
        }
        match min_on_s {
            Some(m) => {
                sets.push("#m = :m");
                request =
                    request.expression_attribute_values(":m", AttributeValue::N(m.to_string()));
            }
            None => removes.push("#m"),
        }
        let mut expression = String::new();
        if !sets.is_empty() {
            expression.push_str("SET ");
            expression.push_str(&sets.join(", "));
        }
        if !removes.is_empty() {
            if !expression.is_empty() {
                expression.push(' ');
            }
            expression.push_str("REMOVE ");
            expression.push_str(&removes.join(", "));
        }
        let request = request.update_expression(expression);

        match block_on(request.send()) {
            Ok(out) => {
                let item = out.attributes().ok_or_else(|| {
                    AppError::Store(StoreError::io(
                        "set_device_threshold",
                        "no attributes returned",
                    ))
                })?;
                Self::item_to_device(item)
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_conditional_check_failed_exception() {
                    Err(AppError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                } else {
                    Err(AppError::Store(StoreError::io(
                        "set_device_threshold",
                        service_err,
                    )))
                }
            }
        }
    }

    fn set_desired_fw(
        &self,
        device_id: &str,
        desired_fw: Option<&str>,
    ) -> Result<DeviceRecord, AppError> {
        let base = self
            .client
            .update_item()
            .table_name(&self.devices_table)
            .key("device_id", AttributeValue::S(device_id.to_string()))
            .expression_attribute_names("#f", "desired_fw")
            .condition_expression("attribute_exists(device_id)")
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew);
        let request = match desired_fw {
            Some(fw) => base
                .update_expression("SET #f = :f")
                .expression_attribute_values(":f", AttributeValue::S(fw.to_string())),
            None => base.update_expression("REMOVE #f"),
        };

        match block_on(request.send()) {
            Ok(out) => {
                let item = out.attributes().ok_or_else(|| {
                    AppError::Store(StoreError::io("set_desired_fw", "no attributes returned"))
                })?;
                Self::item_to_device(item)
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_conditional_check_failed_exception() {
                    Err(AppError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                } else {
                    Err(AppError::Store(StoreError::io(
                        "set_desired_fw",
                        service_err,
                    )))
                }
            }
        }
    }

    fn set_device_style(
        &self,
        device_id: &str,
        style: DeviceStyle,
    ) -> Result<DeviceRecord, AppError> {
        // One expression: SET the parts that are present, REMOVE the rest.
        let mut sets = Vec::new();
        let mut removes = Vec::new();
        let mut request = self
            .client
            .update_item()
            .table_name(&self.devices_table)
            .key("device_id", AttributeValue::S(device_id.to_string()))
            .condition_expression("attribute_exists(device_id)")
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew);
        for (i, (name, value)) in Self::style_attributes(&style).into_iter().enumerate() {
            let placeholder = format!("#s{i}");
            request = request.expression_attribute_names(&placeholder, name);
            match value {
                Some(v) => {
                    let value_key = format!(":s{i}");
                    sets.push(format!("{placeholder} = {value_key}"));
                    request = request.expression_attribute_values(value_key, v);
                }
                None => removes.push(placeholder),
            }
        }
        let mut expression = String::new();
        if !sets.is_empty() {
            expression.push_str("SET ");
            expression.push_str(&sets.join(", "));
        }
        if !removes.is_empty() {
            if !expression.is_empty() {
                expression.push(' ');
            }
            expression.push_str("REMOVE ");
            expression.push_str(&removes.join(", "));
        }
        let request = request.update_expression(expression);

        match block_on(request.send()) {
            Ok(out) => {
                let item = out.attributes().ok_or_else(|| {
                    AppError::Store(StoreError::io("set_device_style", "no attributes returned"))
                })?;
                Self::item_to_device(item)
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_conditional_check_failed_exception() {
                    Err(AppError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                } else {
                    Err(AppError::Store(StoreError::io(
                        "set_device_style",
                        service_err,
                    )))
                }
            }
        }
    }

    fn latest_uplinks(&self) -> Result<Vec<UplinkRecord>, AppError> {
        let devices = self.list_devices()?;
        let mut out = Vec::with_capacity(devices.len());
        for device in devices {
            if let Some(uplink) = self.get_uplinks(&device.device_id, 1)?.into_iter().next() {
                out.push(uplink);
            }
        }
        Ok(out)
    }

    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError> {
        let result = block_on(
            self.client
                .query()
                .table_name(&self.messages_table)
                .key_condition_expression("device_id = :did")
                .expression_attribute_values(":did", AttributeValue::S(device_id.to_string()))
                .scan_index_forward(false)
                .limit(query_limit(limit))
                .send(),
        )
        .map_err(|e| AppError::Store(StoreError::io("get_uplinks", e)))?;

        result
            .items()
            .iter()
            .map(|item| item_to_uplink(item, "get_uplinks"))
            .collect()
    }

    /// Range query on the sort key: `received_at >= :since`, ascending. One
    /// page of up to `limit` items — a window bigger than one page is
    /// reported truncated by the caller (`fleet::history`), not followed.
    fn device_uplinks_since(
        &self,
        device_id: &str,
        since: &str,
        limit: usize,
    ) -> Result<Vec<UplinkRecord>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let result = block_on(
            self.client
                .query()
                .table_name(&self.messages_table)
                .key_condition_expression("device_id = :did AND received_at >= :since")
                .expression_attribute_values(":did", AttributeValue::S(device_id.to_string()))
                .expression_attribute_values(":since", AttributeValue::S(since.to_string()))
                .scan_index_forward(true)
                .limit(query_limit(limit))
                .send(),
        )
        .map_err(|e| AppError::Store(StoreError::io("device_uplinks_since", e)))?;

        result
            .items()
            .iter()
            .map(|item| item_to_uplink(item, "device_uplinks_since"))
            .collect()
    }

    /// Per-device range queries merged by `received_at`, then cut to the
    /// oldest `limit` rows overall — same contract as the SQLite query.
    fn uplinks_since(&self, since: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError> {
        let devices = self.list_devices()?;
        let mut out = Vec::new();
        for device in devices {
            out.extend(self.device_uplinks_since(&device.device_id, since, limit)?);
        }
        out.sort_by(|a, b| {
            a.received_at
                .cmp(&b.received_at)
                .then_with(|| a.device_id.cmp(&b.device_id))
        });
        out.truncate(limit);
        Ok(out)
    }
}

/// DynamoDB takes an i32 page limit; clamp instead of wrapping a huge usize
/// into a negative (which the service rejects).
fn query_limit(limit: usize) -> i32 {
    i32::try_from(limit).unwrap_or(i32::MAX).max(1)
}

fn item_to_uplink(
    item: &std::collections::HashMap<String, AttributeValue>,
    op: &'static str,
) -> Result<UplinkRecord, AppError> {
    let device_id = item
        .get("device_id")
        .and_then(|v| v.as_s().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Store(StoreError::io(op, "missing device_id")))?;
    let received_at = item
        .get("received_at")
        .and_then(|v| v.as_s().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Store(StoreError::io(op, "missing received_at")))?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_device(id: &str) -> DeviceRecord {
        DeviceRecord {
            device_id: id.into(),
            owner_id: "owner-1".into(),
            subject_dn: None,
            status: "active".into(),
            created_at: "2025-01-01T00:00:00Z".into(),
            power_threshold_ma: None,
            min_on_s: None,
            style: Default::default(),
            desired_fw: None,
        }
    }

    #[test]
    fn device_to_item_roundtrip() {
        let device = make_device("dev-1");
        let item = DynamoDeviceStore::device_to_item(&device);
        let recovered = DynamoDeviceStore::item_to_device(&item).unwrap();
        assert_eq!(recovered.device_id, "dev-1");
        assert_eq!(recovered.owner_id, "owner-1");
        assert_eq!(recovered.status, "active");
        assert_eq!(recovered.created_at, "2025-01-01T00:00:00Z");
        assert!(recovered.subject_dn.is_none());
    }

    #[test]
    fn device_to_item_with_subject_dn() {
        let mut device = make_device("dev-2");
        device.subject_dn = Some("CN=device2,O=supervictor".into());
        let item = DynamoDeviceStore::device_to_item(&device);
        let recovered = DynamoDeviceStore::item_to_device(&item).unwrap();
        assert_eq!(
            recovered.subject_dn.as_deref(),
            Some("CN=device2,O=supervictor")
        );
    }

    #[test]
    fn device_to_item_has_all_fields() {
        let device = make_device("dev-1");
        let item = DynamoDeviceStore::device_to_item(&device);
        assert_eq!(item.len(), 4);
        assert!(item.contains_key("device_id"));
        assert!(item.contains_key("owner_id"));
        assert!(item.contains_key("status"));
        assert!(item.contains_key("created_at"));
    }

    #[test]
    fn device_to_item_includes_subject_dn_when_present() {
        let mut device = make_device("dev-1");
        device.subject_dn = Some("CN=test".into());
        let item = DynamoDeviceStore::device_to_item(&device);
        assert_eq!(item.len(), 5);
        assert!(item.contains_key("subject_dn"));
    }

    #[test]
    fn item_to_device_missing_device_id() {
        let mut item = HashMap::new();
        item.insert("owner_id".into(), AttributeValue::S("o1".into()));
        item.insert("status".into(), AttributeValue::S("active".into()));
        item.insert(
            "created_at".into(),
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        );
        let err = DynamoDeviceStore::item_to_device(&item).unwrap_err();
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn item_to_device_missing_owner_id() {
        let mut item = HashMap::new();
        item.insert("device_id".into(), AttributeValue::S("dev-1".into()));
        item.insert("status".into(), AttributeValue::S("active".into()));
        item.insert(
            "created_at".into(),
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        );
        let err = DynamoDeviceStore::item_to_device(&item).unwrap_err();
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn item_to_device_missing_status() {
        let mut item = HashMap::new();
        item.insert("device_id".into(), AttributeValue::S("dev-1".into()));
        item.insert("owner_id".into(), AttributeValue::S("o1".into()));
        item.insert(
            "created_at".into(),
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        );
        let err = DynamoDeviceStore::item_to_device(&item).unwrap_err();
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn item_to_device_wrong_attribute_type() {
        let mut item = HashMap::new();
        item.insert("device_id".into(), AttributeValue::N("123".into()));
        item.insert("owner_id".into(), AttributeValue::S("o1".into()));
        item.insert("status".into(), AttributeValue::S("active".into()));
        item.insert(
            "created_at".into(),
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        );
        let err = DynamoDeviceStore::item_to_device(&item).unwrap_err();
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn item_to_device_empty_map() {
        let item = HashMap::new();
        let err = DynamoDeviceStore::item_to_device(&item).unwrap_err();
        assert!(format!("{err}").contains("missing field"));
    }
}
