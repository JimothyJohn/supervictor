# supervictor edge

## Commands
```
../../Quickstart                                 # Full pipeline from repo root (lint, build, test)
cargo run --bin supervictor-embedded --features ethernet   # Build + flash via espflash (from this dir; implies chip-esp32)
cargo run --bin supervictor-embedded --features wifi,chip-esp32   # WiFi fallback build (needs SSID/PASSWORD env)
cargo run --bin supervictor-embedded --features wifi,chip-esp32c3 --target riscv32imc-unknown-none-elf   # ESP32-C3 (riscv)
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
`Quickstart install` handles this. The same toolchain builds the ESP32-C3
(riscv) — embedded builds pick exactly one `chip-esp32` / `chip-esp32c3`
feature matched to the `--target` (`ethernet` implies `chip-esp32`; only
the classic ESP32 has an EMAC).

Embedded builds read CERT_PATH/CA_PATH/DEVICE_NAME (the baked certificate)
and API_PATH from the environment at compile time — load them with
`set -a; source ../../.env.dev` first (Quickstart does this automatically).
CERT_PATH is repo-root-relative. HOST, SSID, PASSWORD and DEVICE_NAME (as
the id on the wire) are *runtime* settings since 2026-11: `src/settings/`
reads a checksummed record from the `nvs` partition at boot, and the
values the image was built with only seed an empty partition on first
boot — so a board already in the field keeps them across an OTA, and an
image built without SSID/HOST compiles and boots unprovisioned (halts
with a console line until the provisioning portal lands). Nothing else
reads that partition; the record is not the ESP-IDF NVS format.

## Hardware on the bench
Physical rig (which board is on which port, cabling, flash/monitor
recipes) is documented in `TESTRIG.md` at the repo root — gitignored
because it's machine-specific; if it's missing, there's no hardware
attached to this machine. One portable fact: the `ethernet` feature is
POE-ISO-specific — on the ESP32-POE2 the PHY doesn't respond (MDIO reads
`0xffffffff`; different power/clock wiring than the pin map in
`src/board/poe_iso.rs`), so the POE2 runs the `wifi` build.

Watching a board from a non-TTY context (agent shells): `espflash monitor`
needs `--non-interactive`; add `--chip esp32 --after no-reset --before
no-reset-no-sync` to attach without rebooting the running firmware.

## Provisioning portal (`src/network/portal/`, WiFi builds)
A WiFi board with no SSID in its settings record, or whose previous boot
never associated before the 300 s watchdog reset it, boots as an access
point `supervictor-<device name>` (WPA2; passphrase = last eight hex of
the device certificate's SHA-256 fingerprint — `flash.sh` prints it, so
does the boot console) at `http://192.168.4.1/` with its own DHCP and a
catch-all DNS, so a phone's captive-portal detector opens the form. The
form takes SSID, password, host and device name; the board joins and
completes an mTLS handshake with the host *before* writing the record,
then reboots as a station. A configured-but-unreachable network gets
retried after 10 min with nobody on the portal. `boot.rs` (RTC record
policy), `page.rs`, `dhcp.rs` and `psk.rs` are pure and host-tested;
`mod.rs` holds the tasks. The watchdog is fed from the portal's accept
loop. Not the IDF NVS format; the record is `src/settings/record.rs`.

## Pushed settings (`config` in the uplink ack)
`UplinkAck.config` (`models/uplink.rs::AckConfig`) is the endpoint's
settings block: `version`, `uplink_interval_s`, `threshold_ma`. The uplink
loop keeps a live copy of the settings record; `Settings::apply_pushed`
takes a block whose version differs from the one on flash (interval
clamped to 1–600 s via `supervictor_common::config`, 0 = compiled
`UPLINK_INTERVAL_S`), the loop re-times itself, writes the record through
`Ota::with_flash` and reports `config_version` in every uplink after. The
version is a pure function of the values on the endpoint side, so there
is nothing to reconcile; a board without a usable partition applies the
block until reboot and still echoes it.

## Network interfaces
`embedded` is the interface-less base; the bin needs exactly one of:
- `ethernet` (primary) — wired EMAC + LAN8710A over RMII, PoE-powered.
  Board bring-up (APLL 50 MHz + GPIO17 clock-out, pin map) lives in
  `src/board/poe_iso.rs`; the driver/stack wiring in `src/network/ethernet.rs`.
  The EMAC driver is `ph-esp32-mac` pinned to the upstream merge commit of
  the MDIO-read fix (photon-circus/ph-esp32-mac#4); no release carries it yet.
- `wifi` (fallback) — esp-radio station mode.

## no_std Constraints
- Library code is `#![no_std]` — no `String`, `Vec`, `format!`, or `std::` imports
- Use `heapless::String<N>` (aliased as `HString<N>`) for strings
- Use `serde-json-core` for JSON serialization (not `serde_json`)
- Buffer sizes and capacities are defined in `src/config.rs` — check before changing message formats
- The wire twin `models/uplink.rs` carries an optional `levels` burst: `UPLINK_BURST`
  samples per interval, one byte per register, quantised with `supervictor_common::levels`
  (the one shared dependency); default 50 per 5 s = 10 Hz. `uplink_tests::full_uplink_with_max_id_fits_the_request_buffers`
  pins the JSON (1536) / request (2048) buffers in `network/http.rs`.

## Telemetry simulator (feature `sim`, off by default)
- `src/models/sim.rs` fills the uplink; `src/models/profile.rs` makes it act
  like a machine (`Profile::parse` lists the names). Integer-only, seeded,
  stepped one simulated second at a time; tests run a full day per profile.
- Behind `--features sim` (decided 2026-10-27): a production image can never
  report a reading it did not take. Without it the uplink carries `current`
  0 (no sensor driver yet, HARDWARE.md §7), `seq`, `uptime_s`, `interval_s`,
  `heap_free` and nothing invented. `scripts/flash.sh` / `ota-push.sh` add
  `sim` unless `SIM=0`; `desktop` implies it; tests always compile it.
  `src/models/rng.rs` (xorshift, seed, uplink jitter) stays in every build.
- `SIM_PROFILE` / `SIM_SPEED`: compile-time (`option_env!`) for the firmware,
  runtime env for `supervictor-desktop`. Unknown profile = compile error.
- Fleet of containers: `docker-compose.sim.yml` + `Dockerfile.desktop`
  (context = repo root, so edge/.cargo and the nightly toolchain file don't
  apply); `../../scripts/sim-fleet.sh up staging` issues certs, registers,
  starts; `down` stops the containers (they restart with Docker otherwise).

## Key Patterns
- **Async runtime**: Embassy — use `embassy_time::Timer`, never `std::thread::sleep`
- **TLS**: `mbedtls-rs` (embedded), `rustls` (desktop)
