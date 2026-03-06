"""Tests verifying the device firmware source correctly uses env!("CA_PATH").

These are source-level verification tests — they read the actual Rust source
files and verify the env!("CA_PATH") wiring is correct across config.rs,
tls.rs, and build.rs. Catches regressions if someone reverts to hardcoded paths.
"""

from __future__ import annotations

import re
from pathlib import Path

DEVICE_DIR = Path(__file__).resolve().parents[2] / "device"


class TestConfigRs:
    """Verify config.rs reads CA_PATH from environment."""

    def test_ca_path_uses_env_macro(self):
        config_rs = (DEVICE_DIR / "src" / "config.rs").read_text()
        assert 'env!("CA_PATH")' in config_rs, (
            "config.rs must read CA_PATH from compile-time environment"
        )

    def test_ca_path_is_pub_const(self):
        config_rs = (DEVICE_DIR / "src" / "config.rs").read_text()
        pattern = r'pub\s+const\s+CA_PATH\s*:\s*&str\s*=\s*env!\("CA_PATH"\)'
        assert re.search(pattern, config_rs), (
            'CA_PATH must be declared as `pub const CA_PATH: &str = env!("CA_PATH")`'
        )

    def test_no_hardcoded_amazon_root_ca(self):
        config_rs = (DEVICE_DIR / "src" / "config.rs").read_text()
        assert "AmazonRootCA1" not in config_rs, (
            'config.rs must not hardcode AmazonRootCA1 — use env!("CA_PATH")'
        )


class TestTlsRs:
    """Verify tls.rs loads the CA cert via env!("CA_PATH")."""

    def test_include_str_uses_env_ca_path(self):
        tls_rs = (DEVICE_DIR / "src" / "network" / "tls.rs").read_text()
        assert 'include_str!(env!("CA_PATH"))' in tls_rs, (
            'tls.rs must use include_str!(env!("CA_PATH")) for CA cert loading'
        )

    def test_no_hardcoded_ca_include(self):
        tls_rs = (DEVICE_DIR / "src" / "network" / "tls.rs").read_text()
        assert "AmazonRootCA1" not in tls_rs, "tls.rs must not hardcode AmazonRootCA1.pem path"

    def test_ca_chain_still_null_terminated(self):
        """The concat! must still append a null byte for X509::pem."""
        tls_rs = (DEVICE_DIR / "src" / "network" / "tls.rs").read_text()
        # Should be: concat!(include_str!(env!("CA_PATH")), "\0")
        pattern = r'concat!\(\s*include_str!\(env!\("CA_PATH"\)\)\s*,\s*"\\0"\s*\)'
        assert re.search(pattern, tls_rs), (
            'CA cert must be null-terminated: concat!(include_str!(env!("CA_PATH")), "\\0")'
        )


class TestBuildRs:
    """Verify build.rs tracks CA_PATH for recompilation."""

    def test_ca_path_in_rerun_list(self):
        build_rs = (DEVICE_DIR / "build.rs").read_text()
        assert '"CA_PATH"' in build_rs, "build.rs must include CA_PATH in rerun-if-env-changed list"

    def test_rerun_if_env_changed_emitted(self):
        """The rerun loop must actually emit cargo:rerun-if-env-changed."""
        build_rs = (DEVICE_DIR / "build.rs").read_text()
        assert "cargo:rerun-if-env-changed" in build_rs

    def test_all_env_vars_tracked(self):
        """Every env!() var used in the crate should be in the rerun list."""
        build_rs = (DEVICE_DIR / "build.rs").read_text()

        # Extract the var list from the for loop
        list_match = re.search(r"for var in \[([^\]]+)\]", build_rs)
        assert list_match, "Could not find env var list in build.rs"
        tracked = set(re.findall(r'"(\w+)"', list_match.group(1)))

        # These are the env!() vars used across the crate
        required = {"HOST", "DEVICE_NAME", "CA_PATH"}
        missing = required - tracked
        assert not missing, f"build.rs missing rerun tracking for: {missing}"


class TestCargoConfig:
    """Verify .cargo/config.toml doesn't conflict with env-based CA_PATH."""

    def test_no_hardcoded_ca_path_in_cargo_config(self):
        config_toml = (DEVICE_DIR / ".cargo" / "config.toml").read_text()
        assert "AmazonRootCA1" not in config_toml, ".cargo/config.toml must not hardcode CA path"

    def test_no_ca_path_env_override(self):
        """If .cargo/config.toml sets CA_PATH, it defeats the purpose of env!()."""
        config_toml = (DEVICE_DIR / ".cargo" / "config.toml").read_text()
        # CA_PATH should NOT be in the [env] section — it comes from .env.dev
        assert "CA_PATH" not in config_toml, (
            ".cargo/config.toml must not set CA_PATH — it should come from .env.dev"
        )
