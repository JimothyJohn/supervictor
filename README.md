![supervictor](docs/banner.jpg)

# supervictor

**One language. One toolchain. Sensor to cloud.**

Supervictor is an all-Rust IoT framework that covers the entire stack — bare-metal firmware on ESP32-family microcontrollers (the Olimex ESP32-POE-ISO and ESP32-POE2 over wired PoE Ethernet, the ESP32-C3 over WiFi), a cloud API with a live fleet dashboard, and mTLS certificate management — wired together by a single `./Quickstart` entry point. Other MCU families are a welcome future port. No Node.js lambdas. No YAML-driven code generators. Rust from the register to the database; one deliberate shell script for orchestration.

## Why One Language Matters

Most IoT stacks are a Frankenstein of C firmware, Python cloud functions, bash deploy scripts, and JavaScript dashboards — held together by JSON contracts that nobody validates. When the firmware team changes a field name, the cloud team finds out at 2 AM.

Supervictor takes a different approach:

- **Shared types across the entire stack.** The same `UplinkMessage` struct compiles into the firmware and the API. Change a field and the compiler catches every callsite — on the microcontroller *and* in the cloud — before anything ships.
- **One build system.** `cargo build` works for the firmware and the API. No polyglot toolchain to install, no version matrix to maintain.
- **One test runner.** Unit tests, integration tests, and end-to-end mTLS verification all run with `cargo test`. Same language, same assertions, same CI pipeline.
- **One dependency tree to audit.** Security review one ecosystem instead of three. `cargo audit` covers your firmware, your API, and your deploy tooling in a single pass.
- **Refactor without fear.** Rename a function, restructure a module, change a protocol — the compiler tells you exactly what broke across every layer. Try that across C, Python, and JavaScript.

The result: an IoT stack where a solo developer moves as fast as a team, and a team moves as fast as a well-oiled machine.

## Architecture

```
 ESP32 board                  mTLS                     Endpoint
┌──────────────────┐    ┌──────────────┐    ┌──────────────────────────┐
│  Rust/no_std     │───>│   X.509      │───>│  Rust/axum/tokio         │
│  Embassy async   │    │   client     │    │  mTLS terminated in-     │
│  PoE Ethernet    │    │   certs      │    │  binary (rustls)         │
│  or WiFi         │    └──────────────┘    │  SQLite / DynamoDB       │
└──────────────────┘            ▲           │  /ui dashboard, /fleet   │
      edge/                     │           └──────────────────────────┘
                                │                    endpoint/
┌──────────────────┐            │
│  Simulated       │────────────┘   Same client, same certs: a
│  machines        │                container per pretend machine.
│  (desktop client)│
└──────────────────┘
```

Three crates (`common`, `edge`, `endpoint`). The wire contract — routes, field names, status values — lives in `common` and is compiled into both ends.

## Quick Start

```bash
git clone git@github.com:JimothyJohn/supervictor.git
cd supervictor

# Generate mTLS certificates (fleet CA, one device, one server, one admin)
./Quickstart certs ca
./Quickstart certs device esp32
./Quickstart certs server localhost
./Quickstart certs admin you

# Run the endpoint locally (in-binary mTLS, SQLite) and open the dashboard
./scratch/local-ui.sh            # https://localhost:8443/ui with the admin cert

# Flash a board, or run a simulated fleet instead of hardware
./Quickstart onboard <device> <owner> staging.supervictor.advin.io
./scripts/sim-fleet.sh up staging
```

## Repository Layout

```
supervictor/
  supervictor/
    common/                        # Shared wire contract: types, routes, field names (no_std)
    edge/                          # Firmware + desktop client (no_std core, Embassy async)
      src/bin/embedded_main.rs     #   firmware entry point (ESP32 xtensa / ESP32-C3 riscv)
      src/bin/desktop_main.rs      #   desktop mTLS client — same simulator, runs anywhere
      src/board/                   #   Olimex POE-ISO / POE2 pin maps, RMII clock bring-up
      src/network/                 #   HTTP, TLS, DNS, Ethernet, WiFi
      src/models/                  #   uplink message, telemetry simulator, machine profiles
      Dockerfile.desktop           #   container image for the simulated fleet
      docker-compose.sim.yml       #   ten simulated machines, one container each
    endpoint/                      # Cloud API + dashboard (axum + tokio)
      src/handlers.rs              #   framework-agnostic pure functions
      src/routes.rs                #   axum router
      src/fleet.rs, history.rs     #   staleness, power state, bucketed history
      src/ui/                      #   server-rendered dashboard (maud SSR + SSE)
      src/store/                   #   pluggable backends (SQLite, DynamoDB)
      src/tls.rs                   #   in-binary mTLS listener
      deploy/                      #   systemd unit + installer for the single-binary path
      template.yaml                #   SAM/CloudFormation (Lambda path)
  scripts/                         # install-endpoint.sh, sim-fleet.sh, staging deploy
  certs/                           # generated certs (gitignored)
  docs/                            # landing page, TELEMETRY.md wire schema
```

## Fleet dashboard

`/ui` (admin certificate required) is a server-rendered dashboard with a live event stream:

- **Fleet table** — every registered device with its status, freshness (fresh / stale / dark), last current and power badge. Rows update in place as uplinks arrive.
- **Power state** — a device is *on* when its last current is at or above its threshold. The fleet default is `POWER_THRESHOLD_MA`; any device can carry its own bar (set from its page or `PUT /devices/{id}/threshold`), so "on" can mean *producing* for a press whose idle pump draws 9 A and *powered* for a bench board.
- **History** — `/ui/history?window=` charts current, temperature, humidity, RSSI and heap for the whole fleet, plus status-over-time and power-over-time timelines. Windows: `1m`, `5m`, `15m`, `1h`, `6h`, `24h`, `7d`.
- **Live windows** — at 5 minutes or shorter the page re-renders on every uplink instead of polling: 5-second buckets, one reading per bucket, charts that move as the plant does. Longer windows reload once a minute.
- **Device pages** — the same charts for one machine, its recent raw uplinks, status revoke/reactivate and the threshold form.
- **JSON twins** — `/fleet`, `/fleet/summary` and `/fleet/history` serve the same data to your alerting and scripts; a watchdog logs devices that go dark.

## Simulated machines

Until real sensors land, every uplink carries a seeded, deterministic simulation — and a board (or the desktop client) can act like a piece of plant equipment:

| `SIM_PROFILE` | Behaviour |
|---|---|
| `generic` (default) | bench trace: baseline with drift, noise, spikes, dropouts |
| `extruder` | barrel heaters soak, then the screw runs while zones cycle; feed starvation, die changes |
| `press-brake` | pump idles between strokes; each bend is a short high-current stroke |
| `cnc` | spindle base load, roughing and finishing passes, rapids, tool changes |
| `saw` | blade idles; cuts creep up as the blade dulls until a blade change |
| `oven` | thermostat bang-bang between two levels; door openings |
| `compressor` | off until cut-in pressure, runs to cut-out; start inrush |
| `conveyor` | light steady load, heavier when loaded; jams stall then trip |
| `molder` | barrel heaters plus a regular clamp / inject / hold / cool / eject cycle |
| `welder` | idle fan, noisy arc-on bursts, fit-up pauses |
| `pump` | steady 24/7 load, setpoint changes, cavitation bursts |

Every profile runs a working day of production runs, breaks and shutdowns (shutdown = 0 mA, which flips the power badge); `current` is the supply current averaged over the uplink interval and `temp_c` is the machine's probe. Firmware takes `SIM_PROFILE` / `SIM_SPEED` at build time (an unknown name fails the build); the desktop client reads them at runtime.

`./scripts/sim-fleet.sh up staging` issues a certificate per machine, registers it with a profile-appropriate power threshold, and starts one hardened container per profile against the endpoint — a whole plant on the dashboard with no hardware attached.

## API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/` | Greeting + mTLS subject (if present) |
| `POST` | `/` | Device uplink — `{ id, current, ...telemetry }` ([schema](docs/TELEMETRY.md)); identity bound to the client certificate |
| `GET` | `/devices` | List registered devices |
| `POST` | `/devices` | Register a device (`device_id`, `owner_id`, `subject_dn`, optional `power_threshold_ma`) |
| `GET` | `/devices/{id}` | Device details |
| `GET` | `/devices/{id}/uplinks` | Recent uplinks |
| `PUT` | `/devices/{id}/threshold` | Set or clear the device's power threshold (admin) |
| `GET` | `/fleet` | Per-device health: staleness, firmware, current, power, threshold (admin) |
| `GET` | `/fleet/summary` | Aggregate counts for alerting (admin) |
| `GET` | `/fleet/history?window=` | Bucketed telemetry + status and power timelines (admin) |
| `GET` | `/ui`, `/ui/history`, `/ui/devices/{id}` | Dashboard (admin); `/ui/events` is the SSE stream |

Handlers are pure functions with zero framework coupling — testable without HTTP, swappable across web frameworks.

## Pipeline

`./Quickstart` orchestrates the full lifecycle through progressive stages:

| Command | What it does |
|---------|-------------|
| `./Quickstart` | Lint + build (host, embedded, desktop) + full test suite |
| `./Quickstart dev` | Endpoint tests + SAM local + integration tests |
| `./Quickstart dev --serve` | Same, but keeps the local server running |
| `./Quickstart edge` | Build + flash firmware |
| `./Quickstart staging` | Dev gate + deploy dev stack + remote integration tests |
| `./Quickstart prod` | Full pipeline + confirmation + production deployment |
| `./Quickstart certs ca\|device\|server\|admin` | mTLS certificate lifecycle |
| `./Quickstart ping` / `fleet` | mTLS health check / fleet status table |
| `./Quickstart onboard` | End-to-end: cert + register + flash + verify |
| `./scripts/sim-fleet.sh up\|status\|thresholds\|down` | The simulated machine fleet |

## Pluggable Storage

The `DeviceStore` trait defines the storage contract. Swap backends without touching a single handler:

```rust
pub trait DeviceStore: Send + Sync {
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError>;
    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError>;
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError>;
    fn set_device_status(&self, device_id: &str, status: &str) -> Result<DeviceRecord, AppError>;
    fn set_device_threshold(&self, device_id: &str, threshold_ma: Option<i64>) -> Result<DeviceRecord, AppError>;
    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError>;
    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
    fn latest_uplinks(&self) -> Result<Vec<UplinkRecord>, AppError>;
    fn uplinks_since(&self, since: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
    fn device_uplinks_since(&self, device_id: &str, since: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
}
```

**Built-in backends:** SQLite (default; additive migrations run on open) and DynamoDB (feature-gated; uplinks expire by TTL). Adding your own is one `impl` block, and the contract suite runs against both.

## Testing

Every layer is testable offline:

```bash
# Edge — the firmware's logic, simulator and machine profiles run on the host
cargo test -p supervictor-edge --lib --target aarch64-apple-darwin

# Endpoint — in-memory SQLite, no AWS
cargo test -p supervictor-endpoint --features sqlite

# Endpoint — the same store contract against a real DynamoDB Local
docker run -d --rm -p 18000:8000 amazon/dynamodb-local:3.3.2
DYNAMODB_LOCAL_ENDPOINT=http://127.0.0.1:18000 cargo test -p supervictor-endpoint --features sqlite,dynamo
```

Device tests use mock TCP servers with canned responses; each machine profile is run for a simulated day and checked for bounds, determinism and its signature behaviour. Endpoint tests use in-memory SQLite via `axum-test`, a real TLS end-to-end with certs from the repo's own generator, and adversarial inputs for every endpoint. No mocked databases.

## Deployment

The endpoint runs anywhere Rust compiles:

- **Any Linux box (the default path):** one static binary that terminates mTLS itself, a systemd unit, SQLite. No Docker, no reverse proxy, no toolchain on the host. The staging reference at `staging.supervictor.advin.io` runs this way on a t4g.micro, with a Let's Encrypt server certificate and the fleet CA for clients.
- **AWS Lambda:** SAM template with Lambda Web Adapter, arm64/Graviton, DynamoDB; API Gateway terminates mTLS via its truststore, ACM certificates, Route53 DNS.
- **Any container platform:** multi-stage Dockerfile, small Debian-slim image.
- **Local dev:** `docker compose up` in `supervictor/endpoint` (Caddy mTLS reverse proxy + SQLite) if you'd rather not run the binary directly.

### Single binary on a Linux server

Releases ship static `aarch64` and `x86_64` musl builds with sha256 sums. On the target machine, as root:

```bash
curl -fsSL https://raw.githubusercontent.com/JimothyJohn/supervictor/master/scripts/install-endpoint.sh \
  | sudo bash -s -- v0.1.0            # downloads, verifies, installs binary + unit
sudo cp server.pem server.key ca.pem /etc/supervictor/   # from ./Quickstart certs
sudo systemctl start supervictor-endpoint
```

With `TLS_CERT`, `TLS_KEY` and `TLS_CLIENT_CA` set (the unit passes the PEMs in as systemd credentials), the binary listens on `:443`, requires a client certificate chaining to `ca.pem`, and takes each device's identity from the certificate it presented. Forwarded-subject headers are ignored in this mode. The unit runs as a dynamic user with `ProtectSystem=strict`, a syscall filter, and only `CAP_NET_BIND_SERVICE`; the database lives in `/var/lib/supervictor`. Re-run the installer with a newer tag to upgrade; `/etc/supervictor/env` (for example `POWER_THRESHOLD_MA`, `REQUIRE_REGISTRATION`) is left alone.

## Tech Stack

| Layer | Crates | Notes |
|-------|--------|-------|
| Device | esp-hal, esp-radio, embassy, mbedtls-rs, ph-esp32-mac, heapless | no_std, async; wired EMAC + LAN8710 PHY or WiFi; RTC watchdog |
| Simulator | (no deps) | integer-only, seeded xorshift; same trace on the ESP32 and the host |
| Endpoint | axum, tokio, rustls, rusqlite, aws-sdk-dynamodb, maud | in-binary mTLS, feature-gated backends, SSR + SSE dashboard |
| Security | X.509 client certs, mTLS everywhere | Zero-trust by default; operator actions audit-logged by certificate identity |

## License

[MIT](LICENSE) — Nick Armenta, 2026
