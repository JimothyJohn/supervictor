"""Tests for quickstart.runner — subprocess wrapper and console output."""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import patch

import pytest

from quickstart import output
from quickstart.runner import confirm, error, run, start_background, step, success


@pytest.fixture(autouse=True)
def _verbose_for_runner_tests() -> None:
    """Enable verbose so step/detail output is visible in tests."""
    output.set_verbose(True)
    output._accent_index = 0


class TestStepOutput:
    """Tests for step() — cyan step header (verbose only)."""

    def test_prints_message(self, capsys: pytest.CaptureFixture[str]) -> None:
        step("Building project")
        captured = capsys.readouterr()
        assert "Building project" in captured.out

    def test_includes_arrow(self, capsys: pytest.CaptureFixture[str]) -> None:
        step("Test")
        captured = capsys.readouterr()
        assert "=>" in captured.out

    def test_suppressed_when_not_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.set_verbose(False)
        step("Hidden")
        assert capsys.readouterr().out == ""


class TestSuccessOutput:
    """Tests for success() — green success message."""

    def test_prints_message(self, capsys: pytest.CaptureFixture[str]) -> None:
        success("All tests passed")
        captured = capsys.readouterr()
        assert "All tests passed" in captured.out


class TestErrorOutput:
    """Tests for error() — red message to stderr."""

    def test_prints_to_stderr(self, capsys: pytest.CaptureFixture[str]) -> None:
        error("Something went wrong")
        captured = capsys.readouterr()
        assert "Something went wrong" in captured.err

    def test_stdout_empty(self, capsys: pytest.CaptureFixture[str]) -> None:
        error("fail")
        captured = capsys.readouterr()
        assert captured.out == ""


class TestRun:
    """Tests for run() — synchronous subprocess execution."""

    def test_dry_run_returns_zero_exit(self) -> None:
        result = run(["echo", "hello"], dry_run=True)
        assert result.returncode == 0

    def test_dry_run_does_not_execute(
        self, capsys: pytest.CaptureFixture[str]
    ) -> None:
        result = run(["echo", "hello"], dry_run=True)
        captured = capsys.readouterr()
        assert "[dry-run]" in captured.out
        assert result.stdout == ""

    def test_verbose_prints_command(self, capsys: pytest.CaptureFixture[str]) -> None:
        run(["echo", "hello"], dry_run=True, verbose=True)
        captured = capsys.readouterr()
        assert "[dry-run]" in captured.out

    def test_capture_returns_stdout(self) -> None:
        result = run(["echo", "hello"], capture=True)
        assert result.stdout.strip() == "hello"

    def test_check_raises_on_failure(self) -> None:
        with pytest.raises(subprocess.CalledProcessError):
            run(["false"], check=True)

    def test_check_false_no_raise(self) -> None:
        result = run(["false"], check=False)
        assert result.returncode != 0

    def test_cwd_changes_directory(self, tmp_path: Path) -> None:
        result = run(["pwd"], capture=True, cwd=tmp_path)
        assert tmp_path.name in result.stdout

    def test_env_passed_to_subprocess(self) -> None:
        import shutil

        sh_path = shutil.which("sh") or "/bin/sh"
        env = {"PATH": "/usr/bin:/bin", "MY_VAR": "test123"}
        result = run(
            [sh_path, "-c", "echo $MY_VAR"],
            capture=True,
            env=env,
        )
        assert "test123" in result.stdout


class TestStartBackground:
    """Tests for start_background() — background process launcher."""

    def test_dry_run_returns_none(self) -> None:
        result = start_background(["sleep", "100"], dry_run=True)
        assert result is None

    def test_dry_run_prints_message(
        self, capsys: pytest.CaptureFixture[str]
    ) -> None:
        start_background(["sleep", "100"], dry_run=True)
        captured = capsys.readouterr()
        assert "[dry-run]" in captured.out

    def test_returns_popen_handle(self) -> None:
        proc = start_background(["sleep", "0.01"])
        assert isinstance(proc, subprocess.Popen)
        proc.wait()

    def test_log_file_captures_output(self, tmp_path: Path) -> None:
        log = str(tmp_path / "out.log")
        proc = start_background(["echo", "logged"], log_file=log)
        assert proc is not None
        proc.wait()
        assert Path(log).read_text().strip() == "logged"

    def test_verbose_prints_command(
        self, capsys: pytest.CaptureFixture[str]
    ) -> None:
        start_background(["sleep", "100"], dry_run=True, verbose=True)
        captured = capsys.readouterr()
        assert "sleep" in captured.out


class TestConfirm:
    """Tests for confirm() — interactive yes/no prompt."""

    def test_yes_returns_true(self) -> None:
        with patch("builtins.input", return_value="y"):
            assert confirm("Continue?") is True

    def test_full_yes_returns_true(self) -> None:
        with patch("builtins.input", return_value="yes"):
            assert confirm("Continue?") is True

    def test_no_returns_false(self) -> None:
        with patch("builtins.input", return_value="n"):
            assert confirm("Continue?") is False

    def test_empty_returns_false(self) -> None:
        with patch("builtins.input", return_value=""):
            assert confirm("Continue?") is False

    def test_eof_returns_false(self) -> None:
        with patch("builtins.input", side_effect=EOFError):
            assert confirm("Continue?") is False

    def test_keyboard_interrupt_returns_false(self) -> None:
        with patch("builtins.input", side_effect=KeyboardInterrupt):
            assert confirm("Continue?") is False

    def test_case_insensitive(self) -> None:
        with patch("builtins.input", return_value="YES"):
            assert confirm("Continue?") is True
