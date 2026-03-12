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
- Tests use in-memory SQLite. No AWS dependencies in tests.
- Same API surface as `cloud/` Python version.
