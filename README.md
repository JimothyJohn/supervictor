![supervictor](docs/banner.jpg)

# supervictor

**One language. One toolchain. Sensor to cloud.**

Supervictor is an all-Rust IoT framework that covers the entire stack — bare-metal firmware on a RISC-V microcontroller, a cloud API, mTLS certificate management, and a CLI that wires it all together. No Python glue scripts. No Node.js lambdas. No YAML-driven code generators. Just Rust, from the register to the database.

## Why One Language Matters

Most IoT stacks are a Frankenstein of C firmware, Python cloud functions, bash deploy scripts, and JavaScript dashboards — held together by JSON contracts that nobody validates. When the firmware team changes a field name, the cloud team finds out at 2 AM.

Supervictor takes a different approach:

- **Shared types across the entire stack.** The same `UplinkMessage` struct compiles into the firmware and the API. Change a field and the compiler catches every callsite — on the microcontroller *and* in the cloud — before anything ships.
- **One build system.** `cargo build` works for the firmware, the API, and the CLI. No polyglot toolchain to install, no version matrix to maintain.
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
     device/                                       endpoint/
                          cli/
                   ┌──────────────────┐
                   │  Build, flash,   │
                   │  deploy, certs,  │
                   │  onboard         │
                   └──────────────────┘
```

Three crates. Three binaries. One language.

## Quick Start

```bash
# Clone and build the CLI
git clone git@github.com:JimothyJohn/supervictor.git
cd supervictor/cli && cargo build --release
alias qs=./target/release/qs

# Generate mTLS certificates
qs certs ca
qs certs device esp32
qs certs server caddy

# Start local endpoint (Docker + Caddy mTLS reverse proxy)
qs dev --serve

# Flash firmware to device
qs edge

# When ready: deploy to production
qs prod
```

## Repository Layout

```
supervictor/
  device/                        # ESP32-C3 firmware (no_std, Embassy async)
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
  cli/                           # qs CLI (clap)
    src/commands/                 #   dev, edge, staging, prod, certs, onboard
  certs/                         # generated certs (gitignored)
  docs/                          # web interface
```

## API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/` | Greeting + mTLS subject (if present) |
| `POST` | `/` | Device uplink — accepts `{ id, current }` |
| `GET` | `/devices` | List all registered devices |
| `POST` | `/devices` | Register a new device |
| `GET` | `/devices/{id}` | Get device details |
| `GET` | `/devices/{id}/uplinks` | Recent uplink history |

Handlers are pure functions with zero framework coupling — testable without HTTP, swappable across web frameworks.

## CLI Pipeline

The `qs` CLI orchestrates the full lifecycle through progressive stages:

| Command | What it does |
|---------|-------------|
| `qs dev` | Unit tests + build endpoint + local server + integration tests |
| `qs dev --serve` | Same, but keeps the server running for manual testing |
| `qs edge` | Build + flash ESP32-C3 firmware |
| `qs staging` | Dev gate + deploy dev stack + remote integration tests |
| `qs prod` | Full pipeline + confirmation + production deployment |
| `qs certs ca\|device\|server` | mTLS certificate lifecycle |
| `qs ping` | mTLS health check against any endpoint |
| `qs onboard` | End-to-end: certs + server + register + flash + verify |

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
# Device — runs on host, not on hardware
cargo test --lib --target aarch64-apple-darwin -p supervictor

# Endpoint — in-memory SQLite, no AWS
cargo test --features sqlite -p supervictor-endpoint

# CLI
cargo test -p supervictor-cli
```

Device tests use mock TCP servers with canned responses. Endpoint tests use in-memory SQLite via `axum-test`. No Docker, no AWS credentials, no network.

## Deployment

The endpoint runs anywhere Rust compiles:

- **Local dev:** `docker compose up` (Caddy mTLS reverse proxy + SQLite)
- **AWS Lambda:** SAM template with Lambda Web Adapter, arm64/Graviton, DynamoDB
- **Any container platform:** Multi-stage Dockerfile, 30MB final image on Debian slim

Production deployment includes mTLS enforcement via API Gateway truststore, ACM certificates, and Route53 DNS.

## Tech Stack

| Layer | Crates | Notes |
|-------|--------|-------|
| Device | esp-hal, embassy, esp-mbedtls, heapless | no_std, async, 144KB heap |
| Endpoint | axum, tokio, rusqlite, aws-sdk-dynamodb | Feature-gated backends |
| CLI | clap, ureq, toml | Sync HTTP, no runtime overhead |
| Security | X.509 client certs, mTLS everywhere | Zero-trust by default |

## License

[MIT](LICENSE) — Nick Armenta, 2026
