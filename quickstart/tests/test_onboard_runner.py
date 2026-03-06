"""Tests for quickstart.commands.onboard_runner."""

from unittest.mock import MagicMock

from quickstart.commands.onboard_runner import run_phases
from quickstart.commands.onboard_types import (
    OnboardContext,
    PhaseResult,
    PhaseStatus,
)
from quickstart.config import ProjectConfig


def _make_ctx(tmp_path):
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
    fn = MagicMock(return_value=result)
    return fn


def test_all_phases_pass(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = _phase(PhaseResult(PhaseStatus.PASSED))
    p2 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0), ("b", p1), ("c", p2)]

    code = run_phases(phases, ctx)

    assert code == 0
    p0.assert_called_once_with(ctx)
    p1.assert_called_once_with(ctx)
    p2.assert_called_once_with(ctx)


def test_phase_fails(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = _phase(PhaseResult(PhaseStatus.FAILED, "boom"))
    p2 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0), ("b", p1), ("c", p2)]

    code = run_phases(phases, ctx)

    assert code == 1
    p0.assert_called_once_with(ctx)
    p1.assert_called_once_with(ctx)
    p2.assert_not_called()


def test_start_at_skips_phases(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = _phase(PhaseResult(PhaseStatus.PASSED))
    p2 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0), ("b", p1), ("c", p2)]

    code = run_phases(phases, ctx, start_at=2)

    assert code == 0
    p0.assert_not_called()
    p1.assert_not_called()
    p2.assert_called_once_with(ctx)


def test_skip_list(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = _phase(PhaseResult(PhaseStatus.PASSED))
    p2 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0), ("b", p1), ("c", p2)]

    code = run_phases(phases, ctx, skip=[1])

    assert code == 0
    p0.assert_called_once_with(ctx)
    p1.assert_not_called()
    p2.assert_called_once_with(ctx)


def test_keyboard_interrupt(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = MagicMock(side_effect=KeyboardInterrupt)
    phases = [("a", p0), ("b", p1)]

    code = run_phases(phases, ctx)

    assert code == 130


def test_api_process_cleanup(tmp_path):
    ctx = _make_ctx(tmp_path)
    proc = MagicMock()
    ctx.api_process = proc
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0)]

    run_phases(phases, ctx)

    proc.terminate.assert_called_once()
    proc.wait.assert_called_once_with(timeout=5)


def test_skipped_phase_result(tmp_path):
    ctx = _make_ctx(tmp_path)
    p0 = _phase(PhaseResult(PhaseStatus.PASSED))
    p1 = _phase(PhaseResult(PhaseStatus.SKIPPED, "not needed"))
    p2 = _phase(PhaseResult(PhaseStatus.PASSED))
    phases = [("a", p0), ("b", p1), ("c", p2)]

    code = run_phases(phases, ctx)

    assert code == 0
    p0.assert_called_once_with(ctx)
    p1.assert_called_once_with(ctx)
    p2.assert_called_once_with(ctx)
