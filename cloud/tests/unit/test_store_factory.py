"""Unit tests for the store factory."""

import sys
from unittest.mock import MagicMock, patch

import pytest
from uplink.store_factory import create_store
from uplink.store_sqlite import SqliteDeviceStore


class TestCreateStore:
    def test_default_creates_sqlite(self) -> None:
        with patch.dict("os.environ", {}, clear=True):
            store = create_store()
        assert isinstance(store, SqliteDeviceStore)

    def test_sqlite_backend(self) -> None:
        with patch.dict("os.environ", {"STORE_BACKEND": "sqlite"}):
            store = create_store()
        assert isinstance(store, SqliteDeviceStore)

    def test_dynamo_backend(self) -> None:
        mock_boto3 = MagicMock()
        with (
            patch.dict("os.environ", {"STORE_BACKEND": "dynamo"}),
            patch.dict(sys.modules, {"boto3": mock_boto3}),
        ):
            # Clear cached module so factory re-imports
            sys.modules.pop("uplink.store_dynamo", None)
            store = create_store()
        mock_boto3.resource.assert_called_once_with("dynamodb")
        mock_table = mock_boto3.resource.return_value.Table
        assert mock_table.call_count == 2

    def test_unknown_backend_raises(self) -> None:
        with patch.dict("os.environ", {"STORE_BACKEND": "redis"}):
            with pytest.raises(ValueError, match="Unknown store backend: redis"):
                create_store()
