"""Tests for quickstart.output — output engine with verbosity control."""

from __future__ import annotations

from unittest.mock import patch

import pytest

from quickstart import output


@pytest.fixture(autouse=True)
def _reset_output_state() -> None:
    """Reset global state between tests."""
    output.set_verbose(False)
    output._accent_index = 0


class TestVerbosityState:
    def test_default_not_verbose(self) -> None:
        assert output.is_verbose() is False

    def test_set_verbose_true(self) -> None:
        output.set_verbose(True)
        assert output.is_verbose() is True

    def test_set_verbose_false_after_true(self) -> None:
        output.set_verbose(True)
        output.set_verbose(False)
        assert output.is_verbose() is False


class TestMilestone:
    def test_always_prints(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.milestone("Building project")
        assert "Building project" in capsys.readouterr().out

    def test_prints_when_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.set_verbose(True)
        output.milestone("Building project")
        assert "Building project" in capsys.readouterr().out

    def test_includes_custom_emoji(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.milestone("Test", emoji="\U0001f9ea ")
        assert "\U0001f9ea" in capsys.readouterr().out

    def test_includes_default_emoji(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.milestone("Test")
        assert "\u2699" in capsys.readouterr().out


class TestStep:
    def test_suppressed_by_default(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.step("Loading env")
        assert capsys.readouterr().out == ""

    def test_visible_when_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.set_verbose(True)
        output.step("Loading env")
        assert "Loading env" in capsys.readouterr().out

    def test_includes_arrow_when_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.set_verbose(True)
        output.step("Test")
        assert "=>" in capsys.readouterr().out


class TestSuccess:
    def test_always_prints(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.success("All passed")
        assert "All passed" in capsys.readouterr().out

    def test_includes_checkmark(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.success("Done")
        assert "\u2705" in capsys.readouterr().out


class TestError:
    def test_prints_to_stderr(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.error("Something broke")
        assert "Something broke" in capsys.readouterr().err

    def test_stdout_empty(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.error("fail")
        assert capsys.readouterr().out == ""

    def test_includes_x_emoji(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.error("fail")
        assert "\u274c" in capsys.readouterr().err


class TestDetail:
    def test_suppressed_by_default(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.detail("$ echo hello")
        assert capsys.readouterr().out == ""

    def test_visible_when_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.set_verbose(True)
        output.detail("$ echo hello")
        assert "echo hello" in capsys.readouterr().out


class TestInfo:
    def test_always_prints(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.info("sam local running at http://localhost:3000")
        assert "sam local running" in capsys.readouterr().out

    def test_prints_when_not_verbose(self, capsys: pytest.CaptureFixture[str]) -> None:
        output.info("visible")
        assert "visible" in capsys.readouterr().out


class TestConfirm:
    def test_yes_returns_true(self) -> None:
        with patch("builtins.input", return_value="y"):
            assert output.confirm("Continue?") is True

    def test_no_returns_false(self) -> None:
        with patch("builtins.input", return_value="n"):
            assert output.confirm("Continue?") is False

    def test_eof_returns_false(self) -> None:
        with patch("builtins.input", side_effect=EOFError):
            assert output.confirm("Continue?") is False

    def test_keyboard_interrupt_returns_false(self) -> None:
        with patch("builtins.input", side_effect=KeyboardInterrupt):
            assert output.confirm("Continue?") is False


class TestAccentCycle:
    def test_rotates_colors(self) -> None:
        n = len(output._ACCENT_CYCLE)
        colors = [output._next_accent() for _ in range(n + 1)]
        # Wraps around
        assert colors[0] == colors[n]
        # Adjacent differ
        assert colors[0] != colors[1]

    def test_all_accents_used(self) -> None:
        n = len(output._ACCENT_CYCLE)
        colors = [output._next_accent() for _ in range(n)]
        assert len(set(colors)) == n
