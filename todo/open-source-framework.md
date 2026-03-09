# Supervictor: Open-Source Framework Roadmap

## Context

Supervictor is a full-stack Rust IoT system (device firmware + cloud API + CLI tooling) that's currently a working but tightly-coupled project built around one specific use case (ESP32-C3 current sensor → AWS Lambda). The goal is to evolve it into a **portable, reusable framework** that others can adopt for their own device-to-cloud projects, with a **unified API** contract across all layers.

The architecture already has strong framework-like bones: pluggable storage (`DeviceStore` trait), framework-agnostic handlers, feature-gated backends, mTLS throughout, and a phase-based CLI orchestrator. But these patterns are implicit and interleaved with supervictor-specific logic.

---

## Workstream 1: Unified API — Shared Types Crate

**Problem**: Device, endpoint, and CLI each define their own models independently. `UplinkMessage` exists in both `device/src/models/uplink.rs` (heapless) and `endpoint/src/models.rs` (std). There's no single source of truth for the API contract.

**Work**:
- Create a `supervictor-types` (or `supervictor-core`) crate at the workspace root
- Define canonical request/response types with conditional compilation: `#[cfg(feature = "std")]` for String, `#[cfg(not(feature = "std"))]` for HString
- Move `UplinkMessage`, `RegisterDeviceRequest`, `DeviceRecord`, `UplinkRecord`, response types here
- Make device, endpoint, and CLI all depend on this shared crate
- Add API version constant (e.g., `API_VERSION = "v1"`)
- Define route path constants (e.g., `DEVICES_PATH`, `UPLINKS_PATH`) so all components agree on URLs

**Establishes**: The **contract** that makes supervictor a framework rather than a project. Anyone building on supervictor knows exactly what messages flow between device and cloud.

---

## Workstream 2: Cargo Workspace

**Problem**: No root `Cargo.toml` workspace. Each crate is standalone with duplicated dependency versions. No shared build configuration.

**Work**:
- Create root `Cargo.toml` with `[workspace]` containing `device/`, `endpoint/`, `cli/`, and the new shared types crate
- Centralize dependency versions via `[workspace.dependencies]` (serde, serde_json, etc.)
- Add workspace-level metadata: description, license, repository, keywords, categories
- Ensure `cargo test --workspace` works (with appropriate target filtering for device)

---

## Workstream 3: Framework Abstraction

**Problem**: The good patterns exist but aren't exposed as extension points. A user who wants to add a new sensor type or storage backend has to understand the whole codebase.

**Work**:

### 3a. Generalize the Device Layer
- Extract the sensor reading into a trait (e.g., `trait Sensor { fn read(&self) -> SensorReading }`) so the firmware loop isn't hardcoded to "current"
- Make the uplink payload generic (`UplinkMessage<T>` where T is the sensor data) instead of fixed `{ id, current }`
- Keep the HTTP/TLS/WiFi stack as reusable modules that any device project can import

### 3b. Generalize the Endpoint Layer
- The `DeviceStore` trait and handler pattern are already excellent — document them as the primary extension points
- Make the uplink payload accept arbitrary JSON (it partially does this via `serde_json::Value` in `UplinkRecord.payload`, but `UplinkMessage` is still rigid)
- Add API versioning to routes (`/v1/devices`, `/v1/health`)
- Consider extracting the router builder so users can mount supervictor routes alongside their own

### 3c. Generalize the CLI
- Make the onboard phase system configurable (users define which phases to run)
- Extract cert management as a standalone library, not just CLI subcommands
- Make `qs` configurable via a project-level config file (e.g., `supervictor.toml`) instead of scattered env vars

---

## Workstream 4: Legacy Cleanup

**Problem**: `cloud/` and `quickstart/` Python directories are superseded but still present. `pyproject.toml` and `uv.lock` reference them. TODOs litter the README.

**Work**:
- Remove `cloud/` and `quickstart/` directories (or move to a `legacy/` branch)
- Remove `pyproject.toml` and `uv.lock`
- Clean up TODO items in README — either complete them or move to GitHub Issues
- Resolve all `TODO`/`FIXME` comments in device source code
- Complete the `refactor/esp-hal-1.0` branch work and merge to master

---

## Workstream 5: Open-Source Readiness

**Problem**: The project has an MIT license but little else that an outside contributor needs.

### 5a. Documentation (HIGH priority)
- **README.md**: Complete rewrite — what supervictor is, why it exists, architecture diagram, quick-start, build instructions per component, deployment options, link to docs
- **docs/**: Expand beyond the single `index.html` — add architecture docs, API reference, deployment guide, "build your first device" tutorial
- **CLAUDE.md files**: These are great internal docs but not visible to external users; distill key patterns into public docs

### 5b. Community Infrastructure (HIGH priority)
- **CONTRIBUTING.md**: Code style (already defined in CLAUDE.md), PR process, testing requirements, branch naming
- **CODE_OF_CONDUCT.md**: Standard Contributor Covenant
- **.github/ISSUE_TEMPLATE/**: Bug report, feature request, device support request
- **.github/PULL_REQUEST_TEMPLATE.md**: Checklist (tests pass, no secrets, docs updated)
- **CHANGELOG.md**: Start tracking changes now, retroactively note major milestones

### 5c. CI/CD (HIGH priority)
- **GitHub Actions workflow**:
  - Lint (clippy + fmt) for all three crates
  - Test endpoint (`cargo test --features sqlite`)
  - Test device (`cargo test --lib --target aarch64-apple-darwin`)
  - Test CLI
  - Build Docker image
  - Pin all action versions with SHA per project conventions
- **Release workflow**: Tag-based releases with changelog generation

### 5d. Packaging
- Add Cargo.toml metadata to all crates (description, license, repository, keywords)
- Publish to crates.io when stable (types crate first, then endpoint, then CLI)
- Docker images to GHCR or Docker Hub

---

## Workstream 6: Portability

**Problem**: Currently tied to ESP32-C3 + AWS Lambda. A "portable framework" should support other targets.

**Work**:
- **Device targets**: Document how to add new boards (ESP32-S3, nRF, RP2040) — the feature-gate pattern (`embedded`/`desktop`) already supports this
- **Cloud targets**: Endpoint already runs anywhere (Docker, bare metal, Lambda). Document deployment options: Docker Compose (local), fly.io, Railway, AWS ECS, Lambda
- **Storage backends**: SQLite and DynamoDB exist. Document how to add new ones (Postgres, Turso, etc.) via the `DeviceStore` trait
- **TLS options**: Support both mTLS (current) and simpler auth (API keys, JWT) for users who don't need mutual TLS
- **Config**: Move from scattered env vars to a unified config file (`supervictor.toml`) with env var overrides

---

## Suggested Priority Order

| Phase | Workstream | Rationale |
|-------|-----------|-----------|
| 1 | Legacy Cleanup (WS4) | Clean foundation before building |
| 2 | Cargo Workspace (WS2) | Structural prerequisite for shared crate |
| 3 | Unified API / Shared Types (WS1) | Defines the framework contract |
| 4 | CI/CD + Community Infra (WS5b, 5c) | Enables contributions |
| 5 | Documentation (WS5a) | Makes the project approachable |
| 6 | Framework Abstraction (WS3) | Generalize for reuse |
| 7 | Portability (WS6) | Broaden the audience |
| 8 | Packaging (WS5d) | Publish when stable |

Phases 1-3 are foundational and should be done before inviting external contributors. Phases 4-5 make the project usable by others. Phases 6-8 are what make it a true framework.

---

## Key Files to Modify/Create

**New files**:
- `/Cargo.toml` (workspace root)
- `/types/Cargo.toml` + `/types/src/lib.rs` (shared types crate)
- `/CONTRIBUTING.md`
- `/CODE_OF_CONDUCT.md`
- `/CHANGELOG.md`
- `/.github/workflows/ci.yml`
- `/.github/ISSUE_TEMPLATE/*.yml`
- `/.github/PULL_REQUEST_TEMPLATE.md`

**Major edits**:
- `/README.md` (rewrite)
- `/device/Cargo.toml` (workspace member, depend on types)
- `/endpoint/Cargo.toml` (workspace member, depend on types)
- `/cli/Cargo.toml` (workspace member, depend on types)
- `/endpoint/src/models.rs` (re-export from types crate)
- `/device/src/models/uplink.rs` (re-export from types crate)
- `/endpoint/src/routes.rs` (add `/v1/` prefix)

**Remove**:
- `/cloud/` (or archive)
- `/quickstart/` (or archive)
- `/pyproject.toml`
- `/uv.lock`

---

## Verification

Each workstream should be verified independently:
- **WS1**: `cargo test --workspace` passes; device and endpoint both import from shared types
- **WS2**: `cargo build --workspace` succeeds (excluding device embedded target)
- **WS3**: A new sensor type or storage backend can be added without modifying framework code
- **WS4**: No Python files remain in the active tree
- **WS5**: GitHub Actions pass on PR; README renders correctly; a new contributor can set up and run tests within 15 minutes
- **WS6**: Endpoint deploys to at least 2 different platforms; device builds for at least 2 targets
