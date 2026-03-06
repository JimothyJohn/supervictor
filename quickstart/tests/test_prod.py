"""Tests for quickstart.commands.prod — truststore reload."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

from quickstart.commands.prod import _reload_truststore


class TestReloadTruststore:
    """Tests for _reload_truststore — API Gateway mTLS truststore refresh."""

    @patch("subprocess.run")
    def test_swaps_uri_to_force_reload(self, mock_run: MagicMock) -> None:
        mock_run.return_value = MagicMock(returncode=0)
        _reload_truststore(verbose=False, dry_run=False)
        # Should call: s3 cp, update (temp), update (canonical), s3 rm
        assert mock_run.call_count == 4
        cmds = [c.args[0] for c in mock_run.call_args_list]
        # First: copy truststore to temp key
        assert "s3" in cmds[0] and "cp" in cmds[0]
        # Second: point domain to temp URI
        assert "update-domain-name" in cmds[1]
        assert any("truststore-reload.pem" in arg for arg in cmds[1])
        # Third: point domain back to canonical URI
        assert "update-domain-name" in cmds[2]
        assert any("truststore.pem" in arg and "reload" not in arg for arg in cmds[2])
        # Fourth: clean up temp key
        assert "s3" in cmds[3] and "rm" in cmds[3]

    @patch("subprocess.run")
    def test_dry_run_skips_subprocess(self, mock_run: MagicMock) -> None:
        _reload_truststore(verbose=False, dry_run=True)
        mock_run.assert_not_called()

    @patch("subprocess.run")
    def test_copy_failure_aborts_early(self, mock_run: MagicMock, capsys) -> None:
        mock_run.return_value = MagicMock(returncode=1, stderr="access denied")
        _reload_truststore(verbose=False, dry_run=False)
        # Only the s3 cp call should happen
        assert mock_run.call_count == 1
        captured = capsys.readouterr()
        assert "failed" in captured.err.lower() or "failed" in captured.out.lower()

    @patch("subprocess.run")
    def test_swap_failure_aborts_before_restore(self, mock_run: MagicMock, capsys) -> None:
        # s3 cp succeeds, first update-domain-name fails
        mock_run.side_effect = [
            MagicMock(returncode=0),
            MagicMock(returncode=1, stderr="bad request"),
        ]
        _reload_truststore(verbose=False, dry_run=False)
        assert mock_run.call_count == 2
