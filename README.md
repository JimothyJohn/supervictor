![supervictor](docs/banner.svg)

# supervictor

Rust from the register to the database. ESP32 firmware, an endpoint that terminates mTLS itself, the certificate authority that issues every identity, and a live fleet dashboard — one Cargo workspace, one static binary on the server. The deployment is a Linux box, systemd and a SQLite file; nothing else runs. What it is for: [supervictor.advin.io](https://www.supervictor.advin.io).

## Quickstart

```bash
git clone https://github.com/JimothyJohn/supervictor.git && cd supervictor
./Quickstart
```

One command, start to finish: it checks the toolchain, writes `.env.dev`, issues the fleet CA and your admin, server and device certificates, builds, runs the test suite, starts the endpoint with mTLS on `https://localhost:8443`, then asks what should post telemetry — a simulated machine on your computer, an ESP32 over USB, or nothing yet. It is idempotent: rerun it after any fix and it skips what already exists. It only prompts on a terminal and only where a choice is ambiguous; `QS_HARDWARE=sim ./Quickstart` answers ahead of time for scripts and CI. `./Quickstart status` and `./Quickstart down` manage what it started.

- `make` is the same path with the usual names (`make help`, `lint`, `test-unit`, `flash`, `deploy`).
- In Claude Code, `/quickstart` runs it with an agent that reads your serial ports and environment first and fixes what fails.
- Rust is the only requirement. Flashing adds `espflash` and the esp toolchain, which `./Quickstart install` sets up.

Open the dashboard by importing `certs/admins/<you>/admin.p12` into your browser (the password printed once when it was issued), or read the same data over curl with the admin certificate.

## One language

- **One wire contract.** `UplinkMessage`, the routes and the field names live in `common/` and compile into the firmware and the API. Rename a field and both ends fail to build until they agree.
- **One toolchain.** `cargo build` and `cargo test` cover firmware, endpoint, simulator and certificate tooling; `cargo audit` covers one dependency tree.
- **One binary to run.** The endpoint links rustls, SQLite and the dashboard. No reverse proxy, no runtime, no managed service between a board and its data.

## Architecture

```
 ESP32 board                  mTLS                     Endpoint
┌──────────────────┐    ┌──────────────┐    ┌──────────────────────────┐
│  Rust/no_std     │───>│   X.509      │───>│  Rust/axum/tokio         │
│  Embassy async   │    │   client     │    │  mTLS terminated in-     │
│  PoE Ethernet    │    │   certs      │    │  binary (rustls)         │
│  or WiFi         │    └──────────────┘    │  SQLite (DynamoDB opt.)  │
└──────────────────┘            ▲           │  /ui dashboard, /fleet   │
      edge/                     │           └──────────────────────────┘
                                │                    endpoint/
┌──────────────────┐            │
│  Simulated       │────────────┘   Same client, same certs: the
│  machine         │                desktop binary, or a container
│  (desktop client)│                per pretend machine.
└──────────────────┘
```

Three crates: `supervictor/common` (wire contract, `no_std`), `supervictor/edge` (firmware and desktop client), `supervictor/endpoint` (API, dashboard, stores, TLS). Boards: Olimex ESP32-POE-ISO and ESP32-POE2 over wired PoE Ethernet, ESP32-C3 over WiFi; other MCUs are a port of `edge/src/board/`.

## Build on it

**Wire contract.** `POST /` carries `{ id, current, ...telemetry }`; everything after `id` and `current` is optional and omitted when absent, units live in field names, and the endpoint stores whatever arrives, so a new field needs no endpoint change. Schema and rules: [docs/TELEMETRY.md](docs/TELEMETRY.md).

**API.** Device routes take the device's certificate; admin routes need a certificate with `OU=admin`.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `POST` | `/` | Device uplink; identity bound to the client certificate |
| `GET`/`POST` | `/devices` | List / register (`device_id`, `owner_id`, `subject_dn`, optional `power_threshold_ma`, `color`, `line_style`, `marker`) |
| `GET` | `/devices/{id}`, `/devices/{id}/uplinks` | Device record, recent uplinks |
| `PUT` | `/devices/{id}/threshold` | Set or clear the device's power threshold (admin) |
| `PUT` | `/devices/{id}/style` | Replace how the dashboard draws the device: `color` (`s0`–`s7` or `#rrggbb`), `line_style`, `marker` (admin) |
| `GET` | `/fleet`, `/fleet/summary` | Per-device health; aggregate counts for alerting (admin) |
| `GET` | `/fleet/history?window=` | Bucketed telemetry plus status and power timelines, `1m`…`7d` (admin) |
| `GET` | `/ui`, `/ui/history?device=…`, `/ui/devices/{id}` | Dashboard — history plots the devices you pick, each in its own colour / line / marker; `/ui/events` is the SSE stream (admin) |

Handlers are pure functions with no framework types; `routes.rs` is the only axum-specific file.

**Storage.** `DeviceStore` in `endpoint/src/store/` is the whole persistence contract: ten methods over devices and uplinks. SQLite is the default (additive migrations on open); DynamoDB is behind the `dynamo` feature with TTL expiry. A new backend is one `impl` block, and the shared contract suite runs against it.

**Endpoint configuration** is environment only: `PORT`, `TLS_CERT` / `TLS_KEY` / `TLS_CLIENT_CA` (all three → mTLS in-process; none → plain HTTP behind a proxy that terminates it), `STORE_BACKEND`, `SQLITE_DB_PATH`, `POWER_THRESHOLD_MA`, `REQUIRE_REGISTRATION`, `WATCHDOG_INTERVAL_SECS`, `UPLINK_TTL_DAYS`. A malformed value fails the boot rather than disabling a check.

**Firmware.** `cargo run --bin supervictor-embedded --features <ethernet | board-poe2 | wifi,chip-esp32c3>` from `supervictor/edge/` builds and flashes. `HOST`, `DEVICE_NAME`, `CERT_PATH`, `CA_PATH`, `SSID`, `PASSWORD` and `TLS_VERSION` are baked in at compile time from `.env.dev`; the device certificate is embedded, so each board is its own identity. `SIM_PROFILE` (`extruder`, `press-brake`, `cnc`, `saw`, `oven`, `compressor`, `conveyor`, `molder`, `welder`, `pump`) makes a board or the desktop client act like that machine; `scripts/sim-fleet.sh up` runs all ten as containers against an endpoint.

## Deploy

The endpoint is one static musl binary. On any Linux box, as root:

```bash
curl -fsSL https://raw.githubusercontent.com/JimothyJohn/supervictor/master/scripts/install-endpoint.sh \
  | sudo bash -s -- v0.1.0                                # verifies the sha256, installs binary + unit
sudo cp server.pem server.key ca.pem /etc/supervictor/   # from ./Quickstart certs
sudo systemctl start supervictor-endpoint
```

It listens on `:443`, requires a client certificate chaining to `ca.pem`, and takes each device's identity from the certificate it presented. The unit runs as a dynamic user with `ProtectSystem=strict`, a syscall filter, `MemoryDenyWriteExecute` and only `CAP_NET_BIND_SERVICE`; the PEMs are passed as systemd credentials and the database lives in `/var/lib/supervictor`. Re-run the installer with a newer tag to upgrade. `make deploy` stands up the staging box this way on a t4g.micro. The same binary also runs as a Lambda behind API Gateway mTLS (`endpoint/template.yaml`) or in a container, if that is where you are.

## Security

- Every connection is mutual TLS 1.2/1.3; every device, server and operator holds its own X.509 identity from the fleet CA. No passwords, tokens or shared keys anywhere in the system.
- Identity comes from the verified peer certificate and nowhere else. Forwarded-subject headers are ignored when TLS is in-process, and an uplink whose certificate does not match the registered `subject_dn` is rejected.
- Revoking a device is a status flip; its certificate stops working at the next request. Operator actions are audit-logged by certificate identity.
- `REQUIRE_REGISTRATION=true` turns away any certificate that is not registered, active and bound to a device record, so an issued-but-unknown identity cannot post.

## Testing and releases

CI runs format, clippy with warnings denied, the full workspace suite, the DynamoDB contract suite against DynamoDB Local, and compile checks for every embedded target on every pull request; green checks are the approval and merges into `dev` land themselves. Tests use real SQLite, real TLS with certificates from the repo's own generator, a simulated day per machine profile, and adversarial inputs on every route — no mocked databases or handshakes. A `v*` tag builds the static `aarch64` and `x86_64` binaries with checksums.
