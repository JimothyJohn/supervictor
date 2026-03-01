"""Tests for runner.run() log_to parameter."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from quickstart import runner


@pytest.fixture
def log_dir(tmp_path: Path) -> Path:
    return tmp_path / "logs"


def test_log_to_captures_stdout(log_dir: Path) -> None:
    """stdout is written to the log file, not the console."""
    log_file = log_dir / "out.log"
    result = runner.run(
        ["echo", "hello from subprocess"],
        log_to=log_file,
    )
    assert log_file.read_text() == "hello from subprocess\n"
    assert result.stdout == "hello from subprocess\n"
    assert result.returncode == 0


def test_log_to_captures_stderr(log_dir: Path) -> None:
    """stderr is appended to the same log file."""
    log_file = log_dir / "err.log"
    result = runner.run(
        ["python3", "-c", "import sys; sys.stderr.write('warn\\n')"],
        log_to=log_file,
    )
    assert "warn" in log_file.read_text()
    assert result.returncode == 0


def test_log_to_raises_on_failure(log_dir: Path) -> None:
    """CalledProcessError is raised when check=True and command fails."""
    log_file = log_dir / "fail.log"
    with pytest.raises(subprocess.CalledProcessError) as exc_info:
        runner.run(
            ["python3", "-c", "import sys; print('output'); sys.exit(1)"],
            log_to=log_file,
            check=True,
        )
    # Log file should still be written before the exception
    assert "output" in log_file.read_text()
    assert exc_info.value.returncode == 1


def test_log_to_no_raise_when_check_false(log_dir: Path) -> None:
    """No exception when check=False even on failure."""
    log_file = log_dir / "nocheck.log"
    result = runner.run(
        ["python3", "-c", "import sys; print('data'); sys.exit(2)"],
        log_to=log_file,
        check=False,
    )
    assert result.returncode == 2
    assert "data" in log_file.read_text()


def test_log_to_creates_parent_dirs(tmp_path: Path) -> None:
    """log_to creates intermediate directories if they don't exist."""
    log_file = tmp_path / "deep" / "nested" / "dir" / "test.log"
    runner.run(["echo", "nested"], log_to=log_file)
    assert log_file.exists()
    assert "nested" in log_file.read_text()


def test_log_to_verbose_still_logs(log_dir: Path, capsys: pytest.CaptureFixture[str]) -> None:
    """With verbose=True, output goes to both console and log file."""
    log_file = log_dir / "verbose.log"
    runner.run(
        ["echo", "visible"],
        log_to=log_file,
        verbose=True,
    )
    assert "visible" in log_file.read_text()
    captured = capsys.readouterr()
    assert "visible" in captured.out


def test_log_to_dry_run_skips(log_dir: Path) -> None:
    """dry_run=True skips execution; no log file is created."""
    log_file = log_dir / "dry.log"
    result = runner.run(
        ["echo", "should not run"],
        log_to=log_file,
        dry_run=True,
    )
    assert result.returncode == 0
    assert not log_file.exists()


def test_without_log_to_no_capture() -> None:
    """Without log_to, stdout/stderr are not captured on the result."""
    result = runner.run(["echo", "direct"])
    # Without capture or log_to, stdout/stderr are None (not captured)
    assert result.stdout is None
