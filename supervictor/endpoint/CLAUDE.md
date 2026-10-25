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
- Fleet history: `GET /fleet/history?window=` (JSON) and `/ui/history`,
  `/ui/devices/{id}?window=` (charts) in `src/history.rs` + `src/ui/charts.rs`.
  `window` is an exact whitelist (15m, 1h, 6h, 24h, 7d; default 1h; else
  422), always 90 buckets, row cap `MAX_ROWS` (response says `truncated`).
  Store support: `uplinks_since` / `device_uplinks_since` (SQLite range on
  `received_at`, DynamoDB sort-key range). Charts are server-rendered SVG
  (no JS chart lib); the inline script only adds hover/keyboard tooltips
  and a visible-tab reload. Colours by fleet slot (`.s0`–`.s7`), validated
  against the dark surface — don't hand-pick new ones.
- Power state: `fleet::power_state` — last `current` ≥ `POWER_THRESHOLD_MA`
  (default 100, any i64) is `on`, below is `off`, no reading or a dark /
  never device is `unknown`. Surfaces on `/fleet` (`current`, `power`),
  `/fleet/summary` (`on`, `off`), `/fleet/history` (`power` timeline,
  `threshold_ma`), the fleet table and the charts (current chart first,
  with the threshold drawn).
- Uplink identity: a device registered with a `subject_dn` is bound to that
  certificate — `POST /` for it must carry a matching mTLS subject or it is
  403'd (`handlers::authorize_uplink`). `REQUIRE_REGISTRATION=true` further
  rejects unregistered, inactive, and unbound (no `subject_dn`) devices;
  with the flag off, unbound registered devices are accepted with a warn
  log. Payload identifiers are
  validated in `validate.rs` (≤64 bytes, non-empty, no control characters —
  the firmware's `HString<64>` contract).
- In-binary mTLS (feature `tls`, default-on; `src/tls.rs`): set `TLS_CERT`,
  `TLS_KEY`, `TLS_CLIENT_CA` (all three or boot fails) and the binary
  terminates TLS itself with mandatory client-cert verification. Identity
  then comes from the verified peer certificate via `ConnectInfo<TlsPeer>`;
  `middleware::client_identity` ignores the forwarded-subject headers in
  that mode. Unset → plain HTTP, identity from the proxy headers (Lambda /
  Caddy). `tests/test_tls.rs` is a real TLS end-to-end with certs minted by
  `scripts/gen_certs.sh` (openssl required).
- Tests use in-memory SQLite by default (no AWS in the default suite).
  The `dynamo` backend has its own contract suite, `tests/test_store_dynamo.rs`,
  against a real DynamoDB Local (`DYNAMODB_LOCAL_ENDPOINT`, see the file
  header); CI runs it in the `dynamo` job with a service container.
- Wire contract (routes, field names) is shared with the firmware via
  `supervictor-common` — change it there, and the compiler finds every
  callsite on both sides.
