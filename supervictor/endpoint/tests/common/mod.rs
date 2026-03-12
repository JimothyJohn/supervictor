use std::sync::Arc;

use supervictor_endpoint::store::sqlite::SqliteDeviceStore;
use supervictor_endpoint::store::DeviceStore;

pub fn test_store() -> Arc<dyn DeviceStore> {
    Arc::new(SqliteDeviceStore::new(":memory:").expect("in-memory sqlite"))
}
