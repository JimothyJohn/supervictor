# Telemetry from your own device

What the endpoint sees is a certificate and a JSON body. Everything below is
a way to produce those two things from different hardware, from "flash the
stock firmware" to "ten lines in any language".

## The wire contract (every path)

`POST https://<HOST>:443/` (or `<API_PATH>` on a raw API Gateway URL), body
`application/json`, over TLS 1.2+ with the device's client certificate:

```json
{"id": "<device id>", "current": 1240}
```

`id` ≤ 64 bytes, no control characters, must equal the registered
`device_id`; `current` is an integer in mA. Everything else is optional and
self-describing by name — `fw`, `seq`, `uptime_s`, `interval_s`,
`heap_free`, `rssi_dbm`, `temp_c`, `humidity_pct`, `accel_g`, `gyro_dps`,
`mag_ut`, and the `levels`/`levels_ms` burst. Full schema and rules:
`docs/TELEMETRY.md`. The endpoint stores whatever arrives, so a new field
needs no server change; floats must be finite.

Cadence: post every `interval_s` seconds (5 by default). A device is
`fresh` under 15 minutes since its last uplink, `stale` under 2 hours,
`dark` after that (`supervictor/endpoint/src/fleet.rs`) — so anything from
once a second to once every ten minutes reads as healthy.

Identity: the certificate's subject must match what was registered. Issue
it with `./Quickstart certs device <id>` (CN = id, OU=devices) and register
as in SKILL.md step 3. One certificate per device; a shared cert is one
device as far as the fleet is concerned.

## Path 1: a supported board (POE-ISO, POE2, ESP32-C3)

Compile-time configuration from `.env.dev` (the firmware bakes these in;
change → rebuild → reflash):

| Variable | Used for |
|---|---|
| `HOST` | DNS name or IPv4 literal the board dials on :443 — never `localhost` |
| `API_PATH` | `/` for a custom domain or single binary; `/<stage>` on a raw execute-api URL |
| `CERT_PATH`, `CA_PATH` | `CERT_PATH + CA_PATH` = the CA that signed the **server** cert; `CERT_PATH/devices/$DEVICE_NAME/client.{pem,key}` = the device identity |
| `DEVICE_NAME` | the `id` on the wire and the cert directory |
| `TLS_VERSION` | minimum TLS version, `1.2` or `1.3` |
| `SSID`, `PASSWORD` | WiFi builds only |
| `UPLINK_INTERVAL_S`, `SIM_PROFILE`, `SIM_SPEED` | optional: cadence, and the machine the simulator impersonates |

Build + flash from `supervictor/edge/` (Quickstart loads `.env.dev` and
`~/export-esp.sh` first; do the same by hand: `set -a; . ../../.env.dev; set +a`):

```sh
cargo run --bin supervictor-embedded --features ethernet                                   # POE-ISO
cargo run --bin supervictor-embedded --features board-poe2                                 # POE2 (Ethernet)
cargo run --bin supervictor-embedded --features wifi,chip-esp32c3 --target riscv32imc-unknown-none-elf   # C3
```

`QS_HARDWARE=flash QS_BOARD=<poe-iso|poe2|c3> ./Quickstart` does the same
with the menu answered. Flashing is USB only (no OTA yet — `docs/OTA.md` is
the plan); PoE boards: never USB and Ethernet at once on the POE2, it has
no galvanic isolation.

## Path 2: another classic-ESP32 Ethernet board (RMII PHY)

The driver stack (`ph-esp32-mac` EMAC + LAN8720/LAN8710 PHY driver +
embassy-net) is board-agnostic; a board is a module in
`supervictor/edge/src/board/` exposing the same surface the two Olimex
modules do, and `board/mod.rs` re-exports the chosen one as `active`:

```rust
pub const PHY_ADDR: u8;            // SMI address the PHY is strapped to
pub const MDC_GPIO: u8;            // SMI clock pin
pub const MDIO_GPIO: u8;           // SMI data pin
pub const PHY_POWER_GPIO: u8;      // pin that gates PHY power (see the limit below)
pub const RMII_CLK_OUT_GPIO: u8;   // pad the 50 MHz reference clock leaves on
pub const RMII_CLK_HZ: u32;        // 50_000_000
pub const XTAL_FREQ_MHZ: u32;      // 40 on production ESP32 modules
pub fn ethernet_mac(base: [u8; 6]) -> [u8; 6];                 // efuse base → EMAC address (+3 is Espressif's scheme)
#[cfg(feature = "ethernet")] pub fn enable_rmii_clk_out(delay: &mut esp_hal::delay::Delay);  // APLL → clock-out pad
```

Steps, each with the file to touch:

1. **Pin map.** From the vendor's Arduino variant or schematic: PHY address,
   MDC/MDIO, PHY power pin, and the reference-clock arrangement. Two cases
   the existing modules already cover: clock **out on GPIO17**
   (`poe_iso.rs`, non-PSRAM modules) or **out on GPIO0** (`poe2.rs`,
   WROVER/PSRAM modules where 16/17 are taken). A board with an external
   50 MHz oscillator feeding GPIO0 as an *input* is a third case: no APLL,
   `RmiiClockMode::ExternalInput` in `network/ethernet.rs` — scope that as
   driver work, not a pin map.
2. **Module.** `board/<name>.rs`: re-export what matches from `poe_iso`
   (the APLL math and `bringup::enable_apll_50mhz` are reusable), override
   the pins; mirror `poe2.rs` for the shape. Add host tests
   `board/<name>_tests.rs` like `poe2_tests.rs` (pin constants and the MAC
   derivation are host-testable; CI runs them).
3. **Selection.** `board/mod.rs`: `pub mod <name>;` and a
   `#[cfg(feature = "board-<name>")] pub use <name> as active;` (keep the
   `cfg(not(...))` default pointing at the POE-ISO).
4. **Feature.** `supervictor/edge/Cargo.toml`: `board-<name> = ["ethernet"]`.
5. **Quickstart menu.** `flash_from_up` in `Quickstart`: add the choice and
   its `features=` case so `QS_BOARD=<name>` works; CI (`.github/workflows/ci.yml`,
   embedded job) gets a `cargo check --features board-<name>` line so a
   refactor cannot silently break the board.
6. **Known limit, say it out loud:** `embedded_main.rs` takes the PHY power
   pin as `peripherals.GPIO12` directly, not from the board module. A board
   whose PHY power is on another pin (or always on) needs that line changed
   or made board-conditional — a small, visible edit, not a hidden one.

Bring-up order and what each log line means is at the top of
`network/ethernet.rs`; `MDIO reads 0xffffffff` means the PHY has no clock
or no power, not a wrong driver.

## Path 3: another ESP32-family chip over WiFi (S3, C6, …)

Not wired today. The build axis is `chip-esp32` / `chip-esp32c3`, each
fanning out to `esp-hal`, `esp-backtrace`, `esp-rtos`, `esp-radio`,
`esp-bootloader-esp-idf` and a `--target`. A new chip means a new
`chip-<x>` feature with those five forwards, the matching target in
`.cargo/config.toml`, a `HEAP_SIZE` in `config.rs`, the `esp_rtos::start`
arm in `embedded_main.rs` (Xtensa vs RISC-V differ), and a CI line.
`esp-hal` is pinned to `=1.0.0` for esp-rtos/esp-radio compatibility, so
check that pin supports the chip before promising anything. Budget it as a
small project with a bench board, not a config change.

## Path 4: Raspberry Pi or any Linux/macOS machine

The desktop client posts the same simulated telemetry with the same
certificate handling, configured at runtime (`supervictor/edge/src/bin/desktop_main.rs`):

```sh
cargo build -p supervictor-edge --features desktop --release        # host; or cross for the Pi
HOST=<HOST> PORT=443 DEVICE_ID=<id> INTERVAL_S=5 \
CA_PEM=certs/ca/ca.pem CLIENT_CERT=certs/devices/<id>/client.pem CLIENT_KEY=certs/devices/<id>/client.key \
  ./target/release/supervictor-desktop
```

`SIM_PROFILE=extruder|press-brake|cnc|saw|oven|compressor|conveyor|molder|welder|pump`
makes it act like that machine; `BURST`, `SIM_SPEED`, `SIM_SEED` as
documented in the file header. `scripts/pi-edge.sh` is the worked example
of cross-building in an arm64 container, copying binary + certs over ssh,
and running detached for N hours — the host and CA there are the bench's;
substitute theirs. To report **real** readings from a Pi, replace the
simulator with their sensor code and keep `post_request`/the TLS setup; or
skip the Rust client entirely and use path 5.

## Path 5: anything that can do mTLS

Prove the contract with curl (SKILL.md step 4), then port those flags.
What every client needs:

- trust store = the CA that signed the server cert (`certs/ca/ca.pem`, or the
  public root if the server uses Let's Encrypt);
- client identity = `client.pem` + `client.key` for the device;
- SNI = `HOST`; HTTP/1.1; `Content-Type: application/json`; `Connection:
  close` is fine (one request per connection is what the firmware does);
- the body above, every `interval_s` seconds; read the response to confirm
  200 and back off on anything else.

Python (stdlib only):

```python
import http.client, json, ssl
ctx = ssl.create_default_context(cafile="certs/ca/ca.pem")
ctx.load_cert_chain("certs/devices/<id>/client.pem", "certs/devices/<id>/client.key")
conn = http.client.HTTPSConnection("<HOST>", 443, context=ctx)
conn.request("POST", "/", json.dumps({"id": "<id>", "current": 1240}), {"Content-Type": "application/json"})
print(conn.getresponse().status)
```

An MCU with its own TLS stack (mbedTLS, wolfSSL, BearSSL) needs the CA and
the device cert/key as PEM or DER blobs, TLS 1.2 at minimum, and about 20 KB
of RAM for the handshake; the ESP32 firmware's `network/tls.rs` and
`network/http.rs` are the reference for buffer sizes (1 KB request, 768 B
JSON) and the request text.
