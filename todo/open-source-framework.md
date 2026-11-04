# Supervictor: Open-Source Framework Roadmap

## Context

Supervictor is a full-stack Rust IoT system (device firmware, an endpoint that terminates mTLS itself, an MCP server, and the `./Quickstart` / `make` tooling) built around one specific use case (ESP32 current sensor → one self-hosted mTLS endpoint). The goal is to evolve it into a **portable, reusable framework** that others can adopt for their own device-to-cloud projects.

The foundation is in place: one Cargo workspace (`supervictor/*`), the wire contract in `supervictor/common` (models and route constants compiled into firmware, endpoint and MCP server), pluggable storage (`DeviceStore` trait), framework-agnostic handlers, feature-gated backends, mTLS throughout, CI with SHA-pinned actions, and tag-built static release binaries. What remains is making those patterns explicit extension points and getting the project ready for outside contributors.

---

## Workstream 1: Workspace polish

- Centralize dependency versions via `[workspace.dependencies]` (serde, serde_json, etc.)
- Add workspace-level metadata: description, license, repository, keywords, categories (only `supervictor-mcp` carries a description today)
- Add an API version constant to `supervictor-common` (e.g., `API_VERSION = "v1"`)

---

## Workstream 2: Framework Abstraction

**Problem**: The good patterns exist but aren't exposed as extension points. A user who wants to add a new sensor type or storage backend has to understand the whole codebase.

### 2a. Generalize the Device Layer
- Extract the sensor reading into a trait (e.g., `trait Sensor { fn read(&self) -> SensorReading }`) so the firmware loop isn't hardcoded to "current"
- Keep the HTTP/TLS/WiFi/Ethernet stack as reusable modules that any device project can import

### 2b. Generalize the Endpoint Layer
- The `DeviceStore` trait and handler pattern are already excellent — document them as the primary extension points
- Add API versioning to routes (`/v1/devices`, `/v1/health`)
- Consider extracting the router builder so users can mount supervictor routes alongside their own

### 2c. Generalize the tooling
- Make the onboard steps configurable (users define which steps to run)
- Extract cert management as a standalone library, not just `./Quickstart certs` subcommands
- Make `./Quickstart` configurable via a project-level config file (e.g., `supervictor.toml`) instead of scattered env vars

---

## Workstream 3: Open-Source Readiness

**Problem**: The project has an MIT license but little else that an outside contributor needs.

### 3a. Documentation
- **docs/**: add a deployment guide and a "build your first device" tutorial beside `TELEMETRY.md`, `OTA.md`, `MESH.md` and `developers.html`
- **CLAUDE.md files**: These are great internal docs but not visible to external users; distill key patterns into public docs

### 3b. Community Infrastructure
- **CONTRIBUTING.md**: Code style, PR process, testing requirements, branch naming
- **CODE_OF_CONDUCT.md**: Standard Contributor Covenant
- **.github/ISSUE_TEMPLATE/**: Bug report, feature request, device support request
- **.github/PULL_REQUEST_TEMPLATE.md**: Checklist (tests pass, no secrets, docs updated)
- **CHANGELOG.md**: Start tracking changes now, retroactively note major milestones

### 3c. CI/CD
- Build the Docker image (`supervictor/endpoint/Dockerfile`) in CI; today nothing does
- Changelog generation on tag-based releases

### 3d. Packaging
- Publish to crates.io when stable (`supervictor-common` first, then the endpoint)
- Docker images to GHCR or Docker Hub

---

## Workstream 4: Portability

**Problem**: Currently tied to the ESP32 family. A "portable framework" should support other targets.

- **Device targets**: The ESP32, ESP32-C3 and ESP32-S3 are wired (`chip-*` features); document how to add new boards (nRF, RP2040) — the `bring-your-own` skill covers ESP32-family and Linux devices today
- **Storage backends**: SQLite and DynamoDB exist. Document how to add new ones (Turso, etc.) via the `DeviceStore` trait
- **TLS options**: Support both mTLS (current) and simpler auth (API keys, JWT) for users who don't need mutual TLS
- **Config**: Move from scattered env vars to a unified config file (`supervictor.toml`) with env var overrides

---

## Suggested Priority Order

| Phase | Workstream | Rationale |
|-------|-----------|-----------|
| 1 | Community Infra (WS3b) | Enables contributions |
| 2 | Documentation (WS3a) | Makes the project approachable |
| 3 | Workspace polish (WS1) | Cheap, and a prerequisite for packaging |
| 4 | Framework Abstraction (WS2) | Generalize for reuse |
| 5 | Portability (WS4) | Broaden the audience |
| 6 | CI/CD + Packaging (WS3c, 3d) | Publish when stable |

---

## Verification

- **WS1**: `cargo build --workspace` succeeds with versions from `[workspace.dependencies]`
- **WS2**: A new sensor type or storage backend can be added without modifying framework code
- **WS3**: A new contributor can set up and run tests within 15 minutes from the README
- **WS4**: Device builds for a non-ESP32 target
