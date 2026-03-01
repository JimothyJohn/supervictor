"""Tests for quickstart.env — .env file parsing and environment merging."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from quickstart.env import load_env, make_env


@pytest.fixture()
def env_file(tmp_path: Path) -> Path:
    """Create a standard .env file for testing."""
    f = tmp_path / ".env"
    f.write_text(
        "HOST=example.com\n"
        "PORT=8080\n"
        "SECRET=hunter2 # inline comment\n"
        'QUOTED="hello world"\n'
        "SINGLE_QUOTED='single'\n"
        "\n"
        "# full line comment\n"
        "EMPTY_VAL=\n"
    )
    return f


class TestLoadEnv:
    """Tests for load_env — bash-style KEY=VALUE parser."""

    def test_parses_simple_key_value(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert result["HOST"] == "example.com"
        assert result["PORT"] == "8080"

    def test_strips_inline_comments(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert result["SECRET"] == "hunter2"

    def test_strips_double_quotes(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert result["QUOTED"] == "hello world"

    def test_strips_single_quotes(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert result["SINGLE_QUOTED"] == "single"

    def test_skips_blank_lines(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert len(result) == 6

    def test_skips_comment_lines(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert not any(k.startswith("#") for k in result)

    def test_empty_value_is_empty_string(self, env_file: Path) -> None:
        result = load_env(env_file)
        assert result["EMPTY_VAL"] == ""

    def test_line_without_equals_skipped(self, tmp_path: Path) -> None:
        f = tmp_path / ".env"
        f.write_text("VALID=yes\nNOEQUALS\n")
        result = load_env(f)
        assert result == {"VALID": "yes"}

    def test_equals_in_value(self, tmp_path: Path) -> None:
        f = tmp_path / ".env"
        f.write_text("URL=https://host?a=1&b=2\n")
        result = load_env(f)
        assert result["URL"] == "https://host?a=1&b=2"

    def test_strips_key_whitespace(self, tmp_path: Path) -> None:
        f = tmp_path / ".env"
        f.write_text("  KEY  =value\n")
        result = load_env(f)
        assert "KEY" in result

    def test_does_not_mutate_os_environ(self, env_file: Path) -> None:
        original = os.environ.copy()
        load_env(env_file)
        assert os.environ == original

    def test_file_not_found_raises(self, tmp_path: Path) -> None:
        with pytest.raises(FileNotFoundError):
            load_env(tmp_path / "nonexistent")


class TestMakeEnv:
    """Tests for make_env — merge dict into os.environ copy."""

    def test_includes_os_environ(self) -> None:
        result = make_env({})
        assert "PATH" in result

    def test_merges_custom_vars(self) -> None:
        result = make_env({"MY_TEST_VAR": "hello"})
        assert result["MY_TEST_VAR"] == "hello"

    def test_overrides_existing_var(self) -> None:
        result = make_env({"PATH": "/custom/path"})
        assert result["PATH"] == "/custom/path"

    def test_does_not_mutate_os_environ(self) -> None:
        original_path = os.environ.get("PATH", "")
        make_env({"PATH": "/custom/path"})
        assert os.environ.get("PATH", "") == original_path

    def test_returns_new_dict(self) -> None:
        result = make_env({})
        assert result is not os.environ
