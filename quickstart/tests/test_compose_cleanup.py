"""Tests for Docker Compose cleanup in onboard_runner.

Verifies the compose_file cleanup path that was added alongside
the api_process cleanup path.
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

from quickstart.commands.onboard_runner import run_phases
from quickstart.commands.onboard_types import OnboardContext, PhaseResult, PhaseStatus
from quickstart.config import ProjectConfig


def _make_ctx(tmp_path: Path) -> OnboardContext:
    config = ProjectConfig.from_repo_root(tmp_path)
    return OnboardContext(
        config=config,
        device_name="test-device",
        owner_id="owner-123",
        mode="onprem",
        verbose=False,
        dry_run=True,
    )


def _phase(result: PhaseResult):
    return MagicMock(return_value=result)


# ── compose_file cleanup ─────────────────────────────────────────────


class TestComposeCleanup:
    def test_compose_down_on_success(self, tmp_path: Path):
        """When compose_file is set, should run docker compose down on exit."""
        ctx = _make_ctx(tmp_path)
        compose_path = tmp_path / "cloud" / "docker-compose.yml"
        compose_path.parent.mkdir(parents=True, exist_ok=True)
        compose_path.touch()
        ctx.compose_file = compose_path

        p0 = _phase(PhaseResult(PhaseStatus.PASSED))

        with patch("subprocess.run") as mock_run:
            run_phases([("a", p0)], ctx)

        mock_run.assert_called_once()
        args = mock_run.call_args[0][0]
        assert args == ["docker", "compose", "-f", str(compose_path), "down"]

    def test_compose_down_on_failure(self, tmp_path: Path):
        """Compose stack should be torn down even if a phase fails."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = tmp_path / "docker-compose.yml"

        p0 = _phase(PhaseResult(PhaseStatus.FAILED, "boom"))

        with patch("subprocess.run") as mock_run:
            code = run_phases([("a", p0)], ctx)

        assert code == 1
        mock_run.assert_called_once()
        args = mock_run.call_args[0][0]
        assert "down" in args

    def test_compose_down_on_interrupt(self, tmp_path: Path):
        """Compose stack should be torn down on KeyboardInterrupt."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = tmp_path / "docker-compose.yml"

        p0 = MagicMock(side_effect=KeyboardInterrupt)

        with patch("subprocess.run") as mock_run:
            code = run_phases([("a", p0)], ctx)

        assert code == 130
        mock_run.assert_called_once()
        assert "down" in mock_run.call_args[0][0]

    def test_compose_takes_precedence_over_api_process(self, tmp_path: Path):
        """When both compose_file and api_process are set, compose wins."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = tmp_path / "docker-compose.yml"
        ctx.api_process = MagicMock()

        p0 = _phase(PhaseResult(PhaseStatus.PASSED))

        with patch("subprocess.run") as mock_run:
            run_phases([("a", p0)], ctx)

        # Should use compose down, not api_process.terminate()
        mock_run.assert_called_once()
        ctx.api_process.terminate.assert_not_called()

    def test_api_process_still_works_without_compose(self, tmp_path: Path):
        """When no compose_file, falls back to api_process cleanup."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = None
        ctx.api_process = MagicMock()

        p0 = _phase(PhaseResult(PhaseStatus.PASSED))

        with patch("subprocess.run") as mock_run:
            run_phases([("a", p0)], ctx)

        # Should NOT call subprocess.run for compose down
        mock_run.assert_not_called()
        # Should terminate the api_process
        ctx.api_process.terminate.assert_called_once()
        ctx.api_process.wait.assert_called_once_with(timeout=5)

    def test_compose_down_with_timeout(self, tmp_path: Path):
        """Compose down should have a timeout to avoid hanging."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = tmp_path / "docker-compose.yml"

        p0 = _phase(PhaseResult(PhaseStatus.PASSED))

        with patch("subprocess.run") as mock_run:
            run_phases([("a", p0)], ctx)

        kwargs = mock_run.call_args[1]
        assert kwargs.get("timeout") == 30

    def test_no_cleanup_when_neither_set(self, tmp_path: Path):
        """When neither compose_file nor api_process is set, no cleanup."""
        ctx = _make_ctx(tmp_path)
        ctx.compose_file = None
        ctx.api_process = None

        p0 = _phase(PhaseResult(PhaseStatus.PASSED))

        with patch("subprocess.run") as mock_run:
            code = run_phases([("a", p0)], ctx)

        assert code == 0
        mock_run.assert_not_called()
