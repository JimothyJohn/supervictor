"""Tests for quickstart.preflight — tool and Docker checks."""

from __future__ import annotations

from unittest.mock import patch

import pytest

from quickstart.preflight import check_docker_running, check_tools, require


class TestCheckTools:
    """Tests for check_tools — verify CLI tools exist on PATH."""

    def test_finds_existing_tool(self) -> None:
        missing = check_tools(["python3"])
        assert "python3" not in missing

    def test_reports_missing_tool(self) -> None:
        missing = check_tools(["nonexistent_tool_xyz_12345"])
        assert "nonexistent_tool_xyz_12345" in missing

    def test_empty_list_returns_empty(self) -> None:
        missing = check_tools([])
        assert missing == []

    def test_mix_of_found_and_missing(self) -> None:
        missing = check_tools(["python3", "nonexistent_tool_xyz_12345"])
        assert "nonexistent_tool_xyz_12345" in missing
        assert "python3" not in missing

    def test_returns_list_type(self) -> None:
        result = check_tools(["python3"])
        assert isinstance(result, list)


class TestCheckDockerRunning:
    """Tests for check_docker_running — Docker daemon probe."""

    def test_returns_false_when_docker_missing(self) -> None:
        with patch("subprocess.run", side_effect=FileNotFoundError):
            assert check_docker_running() is False

    def test_returns_false_when_docker_not_running(self) -> None:
        import subprocess

        with patch("subprocess.run", side_effect=subprocess.CalledProcessError(1, "docker")):
            assert check_docker_running() is False

    def test_returns_true_when_docker_succeeds(self) -> None:
        with patch("subprocess.run"):
            assert check_docker_running() is True


class TestRequire:
    """Tests for require — exit-on-failure preflight gate."""

    def test_exits_on_missing_tool(self) -> None:
        with pytest.raises(SystemExit) as exc_info:
            require(["nonexistent_tool_xyz_12345"])
        assert exc_info.value.code == 1

    def test_no_exit_when_tools_present(self) -> None:
        require(["python3"], need_docker=False)

    def test_exits_when_docker_not_running(self) -> None:
        with patch("quickstart.preflight.check_docker_running", return_value=False):
            with pytest.raises(SystemExit) as exc_info:
                require(["python3"], need_docker=True)
            assert exc_info.value.code == 1

    def test_no_exit_when_docker_running(self) -> None:
        with patch("quickstart.preflight.check_docker_running", return_value=True):
            require(["python3"], need_docker=True)
