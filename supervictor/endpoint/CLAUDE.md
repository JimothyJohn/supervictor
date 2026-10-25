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
  `window` is an exact whitelist (1m, 5m, 15m, 1h, 6h, 24h, 7d; default 1h;
  else 422), row cap `MAX_ROWS` (response says `truncated`). Standard presets
  are 90 buckets; the **live** presets (≤ 5 min: 1m, 5m) bucket at the 5 s
  uplink period (12 / 60 buckets; `Window::buckets`, `Window::grid`) and the
  response says `live: true` + `buckets`. Live pages set `data-live="1"` and
  no `data-refresh`: the SSE script re-renders `<main>` from a fresh GET on
  every uplink (150 ms coalescing, no overlapping fetches, never while a
  form has focus, plus a 15 s tick so the axis moves) and re-binds the chart
  hover via `window.svBindCharts`. Longer windows keep the 60 s visible-tab
  reload. The fleet table is patched in place from the event (`current-now`,
  power badge against the row's `data-threshold`).
  Store support: `uplinks_since` / `device_uplinks_since` (SQLite range on
  `received_at`, DynamoDB sort-key range). Charts are server-rendered SVG
  (no JS chart lib); the inline script only adds hover/keyboard tooltips
  and a visible-tab reload. The history page plots only the devices in the
  `device=` query keys (repeated, never comma-joined: ids may contain
  commas); no key plots nothing, and the chips' hrefs toggle membership so
  the page works without script. The script mirrors the selection into
  `localStorage` and restores it when the URL has no `device` key.
- Device style (`src/style.rs`, wire types in `supervictor_common::models`):
  `DeviceRecord.style` holds an optional colour (`s0`–`s7` palette slot or
  `#rrggbb`), `LineStyle` and `Marker`. `style::resolve` fills gaps —
  devices without a stored colour take palette slots in registration order
  among themselves — and `style::auto_color` gives a new registration the
  first unworn slot. `PUT /devices/{id}/style` and the device-page form
  replace the whole style (omitted parts clear). Charts stroke by colour
  class (`.sN` or `.custom` + inline `--c`), dash class, and per-point
  glyphs; legend/chip/table keys are the same line sample. The palette
  slots are validated for both surfaces — don't hand-pick new ones.
- Bucket grid (`history::Window::grid`): anchored to multiples of the step,
  so a reading never changes bucket between renders; the last bucket is the
  partial one `now` falls in and `until` is its end. Live windows bridge one
  missing bucket in a trace (`charts::LIVE_BRIDGE_BUCKETS`) — jitter at the
  5 s uplink period, not an outage; longer windows bridge nothing.
- Power state: `fleet::power_state` — last `current` ≥ the device's
  threshold is `on`, below is `off`, no reading or a dark / never device is
  `unknown`. The threshold is the device's own `power_threshold_ma`
  (`DeviceRecord::effective_threshold_ma`) when set, else the fleet default
  `POWER_THRESHOLD_MA` (100, any i64). Surfaces on `/fleet` (`current`,
  `power`, `threshold_ma`, `threshold_overridden`), `/fleet/summary` (`on`,
  `off`), `/fleet/history` (`power` timeline per device threshold;
  `threshold_ma` there is the fleet default), the fleet table (`≥ N` beside
  an overridden row) and the charts (current chart first, the device's
  threshold drawn).
- Per-device threshold: `PUT /devices/{id}/threshold` (admin mTLS) with
  `{"power_threshold_ma": 12000}`; `{}`, `null` or an empty body clears it
  (`deny_unknown_fields`, so a misspelt key is 422, never a clear).
  `POST /devices` accepts an optional `power_threshold_ma`. The device page
  has the form (`POST /ui/devices/{id}/threshold`, CSRF). SQLite adds the
  column on open (`ALTER TABLE` guarded by `pragma_table_info`); DynamoDB
  stores it as an `N` attribute, `REMOVE`d on clear. `scripts/sim-fleet.sh`
  sets per-profile values for the simulated machines.
- Live view: `/ui/live` (+ `/ui/live.json` for the polling fallback) shows
  the last 120 s at *sample* resolution as 8-bit register levels
  (`supervictor_common::levels`: 1..=255 spans a fixed range per register,
  0 = absent). `src/live.rs` expands a stored `levels`/`levels_ms` burst
  into timestamped samples (last sample = receive time) or quantises the
  scalar fields into one; the SSE event carries `t_ms`, `levels`,
  `levels_ms`. The page is canvas (10 fps, same `--sN` tokens); the register
  bank's meters are the accessible readout. `validate::check_levels` gates
  ingest; `history::METRICS` is the register table, and `bucket_series`
  folds a burst's earlier samples into their own buckets. Distinct from the
  history page's live *windows* (`1m`/`5m`, `PageMode::Live`), which
  re-render the bucketed charts on every uplink.
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
