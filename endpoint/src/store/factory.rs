use std::sync::Arc;

use crate::config::Config;
use crate::error::AppError;
use crate::store::DeviceStore;

/// Instantiate the configured store backend based on [`Config::store_backend`].
pub async fn create_store(config: &Config) -> Result<Arc<dyn DeviceStore>, AppError> {
    match config.store_backend.as_str() {
        "sqlite" => {
            #[cfg(feature = "sqlite")]
            {
                let store = crate::store::sqlite::SqliteDeviceStore::new(&config.sqlite_db_path)?;
                tracing::info!(path = %config.sqlite_db_path, "sqlite store initialized");
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "sqlite"))]
            Err(AppError::Config("sqlite feature not enabled".into()))
        }
        "dynamo" => {
            #[cfg(feature = "dynamo")]
            {
                let sdk_config =
                    aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                let store = crate::store::dynamo::DynamoDeviceStore::new(
                    &sdk_config,
                    &config.devices_table,
                    &config.messages_table,
                );
                tracing::info!(
                    devices_table = %config.devices_table,
                    messages_table = %config.messages_table,
                    "dynamo store initialized"
                );
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "dynamo"))]
            Err(AppError::Config("dynamo feature not enabled".into()))
        }
        other => Err(AppError::Config(format!("unknown store backend: {other}"))),
    }
}
