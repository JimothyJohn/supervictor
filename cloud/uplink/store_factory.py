"""Store backend factory."""

import os


def create_store():
    """Create a store based on STORE_BACKEND env var."""
    backend = os.environ.get("STORE_BACKEND", "sqlite")
    if backend == "sqlite":
        from uplink.store_sqlite import SqliteDeviceStore

        return SqliteDeviceStore()
    elif backend == "dynamo":
        from uplink.store_dynamo import DynamoDeviceStore

        return DynamoDeviceStore()
    else:
        raise ValueError(f"Unknown store backend: {backend}")
