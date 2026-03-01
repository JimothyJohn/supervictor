"""Tests for qs edge — ESPFLASH_PORT resolution."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from quickstart.commands.edge import run_edge
from quickstart.config import ProjectConfig


@pytest.fixture()
def config(tmp_path: Path) -> ProjectConfig:
    """Minimal ProjectConfig with a fake .env.dev."""
    env_dev = tmp_path / ".env.dev"
    env_dev.write_text("WIFI_SSID=test\n")
    return ProjectConfig(
        repo_root=tmp_path,
        device_dir=tmp_path / "device",
        cloud_dir=tmp_path / "cloud",
        env_dev=env_dev,
        env_staging=tmp_path / ".env.staging",
    )


@pytest.fixture()
def args() -> argparse.Namespace:
    return argparse.Namespace(verbose=False, dry_run=True)


def test_port_from_env_file(config: ProjectConfig, args: argparse.Namespace, tmp_path: Path) -> None:
    """ESPFLASH_PORT in .env.dev is used for --port."""
    config.env_dev.write_text("ESPFLASH_PORT=/dev/ttyUSB0\n")
    with patch("quickstart.commands.edge.runner") as mock_runner:
        mock_runner.run = MagicMock(return_value=subprocess.CompletedProcess([], 0))
        run_edge(args, config)
        call_args = mock_runner.run.call_args
        cmd = call_args[0][0]
        assert "--port" in cmd
        assert "/dev/ttyUSB0" in cmd


def test_port_from_os_env(config: ProjectConfig, args: argparse.Namespace, monkeypatch: pytest.MonkeyPatch) -> None:
    """ESPFLASH_PORT from OS environment is used when not in .env.dev."""
    monkeypatch.setenv("ESPFLASH_PORT", "/dev/cu.usbserial-1420")
    with patch("quickstart.commands.edge.runner") as mock_runner:
        mock_runner.run = MagicMock(return_value=subprocess.CompletedProcess([], 0))
        run_edge(args, config)
        call_args = mock_runner.run.call_args
        cmd = call_args[0][0]
        assert "--port" in cmd
        assert "/dev/cu.usbserial-1420" in cmd


def test_env_file_takes_priority(config: ProjectConfig, args: argparse.Namespace, monkeypatch: pytest.MonkeyPatch) -> None:
    """.env.dev ESPFLASH_PORT wins over OS env."""
    config.env_dev.write_text("ESPFLASH_PORT=/dev/ttyUSB0\n")
    monkeypatch.setenv("ESPFLASH_PORT", "/dev/cu.usbserial-9999")
    with patch("quickstart.commands.edge.runner") as mock_runner:
        mock_runner.run = MagicMock(return_value=subprocess.CompletedProcess([], 0))
        run_edge(args, config)
        call_args = mock_runner.run.call_args
        cmd = call_args[0][0]
        assert "/dev/ttyUSB0" in cmd
        assert "/dev/cu.usbserial-9999" not in cmd


def test_no_port_omits_flag(config: ProjectConfig, args: argparse.Namespace, monkeypatch: pytest.MonkeyPatch) -> None:
    """No ESPFLASH_PORT anywhere means no --port flag."""
    monkeypatch.delenv("ESPFLASH_PORT", raising=False)
    with patch("quickstart.commands.edge.runner") as mock_runner:
        mock_runner.run = MagicMock(return_value=subprocess.CompletedProcess([], 0))
        run_edge(args, config)
        call_args = mock_runner.run.call_args
        cmd = call_args[0][0]
        assert "--port" not in cmd
