# supervictor edge

## Commands
```
../../Quickstart                                 # Full pipeline from repo root (lint, build, test)
cargo run --bin supervictor-embedded --features ethernet   # Build + flash via espflash (from this dir)
cargo run --bin supervictor-embedded --features wifi       # WiFi fallback build (needs SSID/PASSWORD env)
cargo run --bin supervictor-desktop --features desktop     # Desktop mTLS test client

# Host-side test/lint run from the REPO ROOT, not this dir — this dir's
# .cargo/config.toml sets build-std for the xtensa target, which conflicts
# with the prebuilt host std:
cargo test -p supervictor-edge --lib --target aarch64-apple-darwin
cargo clippy -p supervictor-edge --all-targets --target aarch64-apple-darwin   # examples are feature-gated
```
The default target is `xtensa-esp32-none-elf` (Olimex ESP32-POE-ISO). It
needs the esp-rs fork toolchain: `espup install --targets esp32`, then
`. ~/export-esp.sh` so `xtensa-esp32-elf-gcc` (the linker) is on PATH —
`Quickstart install` handles this.

Embedded builds read HOST/CERT_PATH/... (plus SSID/PASSWORD for wifi builds)
from the environment at compile time — load them with
`set -a; source ../../.env.dev` first (Quickstart does this automatically).
CERT_PATH is repo-root-relative.

## Hardware on the bench
Two Xtensa boards (port ↔ board mapping lives in `.env.dev`; ports are
hub-position-dependent, MACs are stable):
- **ESP32-POE-ISO** — runs the `ethernet` build (hardware-verified: PHY id
  `0x0007c0f1`, 100FD link, DHCP, mTLS uplinks).
- **ESP32-POE2** — runs the `wifi` build. The `ethernet` feature does NOT
  work here (PHY id reads `0xffffffff` — different power/clock wiring than
  the POE-ISO pin map in `src/board/poe_iso.rs`).

Watching a board from a non-TTY context (agent shells): `espflash monitor`
needs `--non-interactive`; add `--chip esp32 --after no-reset --before
no-reset-no-sync` to attach without rebooting the running firmware.

## Network interfaces
`embedded` is the interface-less base; the bin needs exactly one of:
- `ethernet` (primary) — wired EMAC + LAN8710A over RMII, PoE-powered.
  Board bring-up (APLL 50 MHz + GPIO17 clock-out, pin map) lives in
  `src/board/poe_iso.rs`; the driver/stack wiring in `src/network/ethernet.rs`.
  The EMAC driver is `ph-esp32-mac` pinned to a git rev carrying the
  MDIO-read fix (upstream PR photon-circus/ph-esp32-mac#4).
- `wifi` (fallback) — esp-radio station mode.

## no_std Constraints
- Library code is `#![no_std]` — no `String`, `Vec`, `format!`, or `std::` imports
- Use `heapless::String<N>` (aliased as `HString<N>`) for strings
- Use `serde-json-core` for JSON serialization (not `serde_json`)
- Buffer sizes and capacities are defined in `src/config.rs` — check before changing message formats

## Key Patterns
- **Async runtime**: Embassy — use `embassy_time::Timer`, never `std::thread::sleep`
- **TLS**: `mbedtls-rs` (embedded), `rustls` (desktop)
