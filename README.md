![supervictor](docs/banner.jpg)

# supervictor

**One language. One toolchain. Sensor to cloud.**

Supervictor is an all-Rust IoT framework that covers the entire stack — bare-metal firmware on a RISC-V microcontroller, a cloud API with a fleet dashboard, and mTLS certificate management — wired together by a single `./Quickstart` entry point. No Node.js lambdas. No YAML-driven code generators. Rust from the register to the database; one deliberate shell script for orchestration.

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
 ESP32-C3                    mTLS                    Axum API
┌──────────────┐       ┌──────────────┐       ┌──────────────────┐
│  Rust/no_std │──────>│   X.509      │──────>│  Rust/tokio      │
│  Embassy     │       │   client     │       │  SQLite/DynamoDB  │
│  esp-mbedtls │       │   certs      │       │  Feature-gated   │
└──────────────┘       └──────────────┘       └──────────────────┘
     edge/                                        endpoint/
                          cli/
                   ┌──────────────────┐
                   │  Build, flash,   │
                   │  deploy, certs,  │
                   │  onboard         │
                   └──────────────────┘
```

Three crates. Two binaries. One language.

## Quick Start

```bash
git clone git@github.com:JimothyJohn/supervictor.git
cd supervictor

# Generate mTLS certificates
./Quickstart certs ca
./Quickstart certs device esp32
./Quickstart certs server caddy

# Start local endpoint (SAM local)
./Quickstart dev --serve

# Flash firmware to device
./Quickstart edge

# When ready: deploy to production
./Quickstart prod
```

## Repository Layout

```
supervictor/
  supervictor/
    common/                        # Shared types & routes (no_std compatible)
    edge/                          # ESP32-C3 firmware (no_std, Embassy async)
      src/bin/embedded_main.rs     #   firmware entry point
      src/bin/desktop_main.rs      #   desktop mTLS test client
      src/app/                     #   application logic + async tasks
      src/network/                 #   HTTP, TLS, DNS, TCP
      src/models/                  #   uplink message types
    endpoint/                      # Cloud API (axum + tokio)
      src/handlers.rs              #   framework-agnostic pure functions
      src/routes.rs                #   axum router
      src/store/                   #   pluggable backends (SQLite, DynamoDB)
      template.yaml                #   SAM/CloudFormation
      docker-compose.yml           #   local dev with Caddy mTLS
  certs/                           # generated certs (gitignored)
  docs/                            # web interface
```

## API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/` | Greeting + mTLS subject (if present) |
| `POST` | `/` | Device uplink — `{ id, current, ...telemetry }` ([schema](docs/TELEMETRY.md)) |
| `GET` | `/devices` | List all registered devices |
| `POST` | `/devices` | Register a new device |
| `GET` | `/devices/{id}` | Get device details |
| `GET` | `/devices/{id}/uplinks` | Recent uplink history |

Handlers are pure functions with zero framework coupling — testable without HTTP, swappable across web frameworks.

## Pipeline

`./Quickstart` orchestrates the full lifecycle through progressive stages:

| Command | What it does |
|---------|-------------|
| `./Quickstart` | Lint + build (host, embedded, desktop) + full test suite |
| `./Quickstart dev` | Endpoint tests + SAM local + integration tests |
| `./Quickstart dev --serve` | Same, but keeps the local server running |
| `./Quickstart edge` | Build + flash ESP32-C3 firmware |
| `./Quickstart staging` | Dev gate + deploy dev stack + remote integration tests |
| `./Quickstart prod` | Full pipeline + confirmation + production deployment |
| `./Quickstart certs ca\|device\|server\|admin` | mTLS certificate lifecycle |
| `./Quickstart ping` / `fleet` | mTLS health check / fleet status table |
| `./Quickstart onboard` | End-to-end: cert + register + flash + verify |

## Pluggable Storage

The `DeviceStore` trait defines the storage contract. Swap backends without touching a single handler:

```rust
pub trait DeviceStore: Send + Sync {
    fn put_device(&self, record: DeviceRecord) -> Result<DeviceRecord, AppError>;
    fn get_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AppError>;
    fn list_devices(&self) -> Result<Vec<DeviceRecord>, AppError>;
    fn put_uplink(&self, record: UplinkRecord) -> Result<(), AppError>;
    fn get_uplinks(&self, device_id: &str, limit: usize) -> Result<Vec<UplinkRecord>, AppError>;
}
```

**Built-in backends:** SQLite (default) and DynamoDB (feature-gated). Adding your own is one `impl` block.

## Testing

Every layer is testable offline with zero external dependencies:

```bash
# Edge — runs on host, not on hardware
cargo test --lib --target aarch64-apple-darwin -p supervictor-edge

# Endpoint — in-memory SQLite, no AWS
cargo test --features sqlite -p supervictor-endpoint

# CLI
cargo test -p supervictor-cli
```

Device tests use mock TCP servers with canned responses. Endpoint tests use in-memory SQLite via `axum-test`. No Docker, no AWS credentials, no network.

## Deployment

The endpoint runs anywhere Rust compiles:

- **Any Linux box (the default path):** one static binary that terminates mTLS itself, a systemd unit, SQLite. No Docker, no reverse proxy, no toolchain on the host. See below.
- **Local dev:** `docker compose up` (Caddy mTLS reverse proxy + SQLite)
- **AWS Lambda:** SAM template with Lambda Web Adapter, arm64/Graviton, DynamoDB; API Gateway terminates mTLS via its truststore, ACM certificates, Route53 DNS.
- **Any container platform:** Multi-stage Dockerfile, 30MB final image on Debian slim

### Single binary on a Linux server

Releases ship static `aarch64` and `x86_64` musl builds with sha256 sums. On the target machine, as root:

```bash
curl -fsSL https://raw.githubusercontent.com/JimothyJohn/supervictor/master/scripts/install-endpoint.sh \
  | sudo bash -s -- v0.1.0            # downloads, verifies, installs binary + unit
sudo cp server.pem server.key ca.pem /etc/supervictor/   # from ./Quickstart certs
sudo systemctl start supervictor-endpoint
```

With `TLS_CERT`, `TLS_KEY` and `TLS_CLIENT_CA` set (the unit passes the PEMs in as systemd credentials), the binary listens on `:443`, requires a client certificate chaining to `ca.pem`, and takes each device's identity from the certificate it presented. Forwarded-subject headers are ignored in this mode. The unit runs as a dynamic user with `ProtectSystem=strict`, a syscall filter, and only `CAP_NET_BIND_SERVICE`; the database lives in `/var/lib/supervictor`. Re-run the installer with a newer tag to upgrade; `/etc/supervictor/env` is left alone.

## Tech Stack

| Layer | Crates | Notes |
|-------|--------|-------|
| Device | esp-hal, embassy, esp-mbedtls, heapless | no_std, async, 144KB heap |
| Endpoint | axum, tokio, rusqlite, aws-sdk-dynamodb | Feature-gated backends |
| CLI | clap, ureq, toml | Sync HTTP, no runtime overhead |
| Security | X.509 client certs, mTLS everywhere | Zero-trust by default |

## License

[MIT](LICENSE) — Nick Armenta, 2026
