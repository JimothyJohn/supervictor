"""Rust toolchain utilities shared across quickstart commands."""

from __future__ import annotations

import subprocess


def host_target() -> str:
    """Return the native Rust host triple (e.g. aarch64-apple-darwin).

    Required because .cargo/config.toml defaults to riscv32imc-unknown-none-elf
    (ESP32-C3), so host-side tests must explicitly target the build machine.
    """
    result = subprocess.run(
        ["rustc", "-vV"],
        capture_output=True,
        text=True,
        check=True,
    )
    for line in result.stdout.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise RuntimeError("Cannot determine Rust host target from `rustc -vV`")
