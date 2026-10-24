# supervictor endpoint

## Executive Summary

Rust API endpoint in `supervictor/endpoint/` — companion to the ESP32 edge device. Uses axum + Lambda Web Adapter for SAM deployment, architected for ECS migration.

## Commands

- `cargo test --features sqlite` — run all tests (unit + integration)
- `cargo build --features sqlite` — build for local dev
- `cargo build --release --features sqlite,dynamo` — build for production
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check
- `docker compose up` — local dev with mTLS via Caddy
- `sam build` — build SAM deployment artifact
- `sam local start-api` — run locally via SAM

## Constraints

- All store operations return `Result<T, AppError>`. Never panic.
- Handlers are framework-agnostic pure functions in `handlers.rs`.
- Store backends are feature-gated: `sqlite` (default) and `dynamo`.
- Fleet dashboard (`src/ui/`, feature `ui`, default-on): maud SSR + SSE at
  `/ui`, admin-only (client-cert subject must carry `OU=admin`). API-only
  builds: `--no-default-features --features sqlite`.
- Fleet health: `GET /fleet` + `/fleet/summary` (JSON, admin mTLS, always
  compiled) share staleness logic with the dashboard via `src/fleet.rs`;
  `src/watchdog.rs` logs dark-device transitions on long-running deploys
  (`WATCHDOG_INTERVAL_SECS`, 0 disables).
- Uplink identity: a device registered with a `subject_dn` is bound to that
  certificate — `POST /` for it must carry a matching mTLS subject or it is
  403'd (`handlers::authorize_uplink`). `REQUIRE_REGISTRATION=true` further
  rejects unregistered/inactive devices; registered devices with no
  `subject_dn` are grandfathered with a warn log. Payload identifiers are
  validated in `validate.rs` (≤64 bytes, non-empty, no control characters —
  the firmware's `HString<64>` contract).
- Tests use in-memory SQLite. No AWS dependencies in tests.
- Wire contract (routes, field names) is shared with the firmware via
  `supervictor-common` — change it there, and the compiler finds every
  callsite on both sides.
