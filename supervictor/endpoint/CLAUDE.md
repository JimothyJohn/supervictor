# supervictor endpoint

## Executive Summary

Rust API endpoint in `supervictor/endpoint/` — companion to the ESP32 edge device. axum, one static binary that terminates mTLS itself, deployed as a systemd unit on any Linux box (`deploy/`, `scripts/install-endpoint.sh`). The SAM/Lambda path was retired 2026-10-27.

## Commands

- `cargo test --features sqlite` — run all tests (unit + integration)
- `cargo build --features sqlite` — build for local dev
- `cargo build --release` — the production binary (sqlite + ui + tls); a v* tag builds the static musl tarballs in CI (`.github/workflows/release.yml`)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — format check
- `docker compose up` — container path: plain HTTP behind Caddy terminating mTLS

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
  `POWER_THRESHOLD_MA` (1000, the clamp's ADC floor; any i64). Surfaces on `/fleet` (`current`,
  `power`, `threshold_ma`, `threshold_overridden`), `/fleet/summary` (`on`,
  `off`), `/fleet/history` (`power` timeline per device threshold;
  `threshold_ma` there is the fleet default), the fleet table (`≥ N` beside
  an overridden row) and the charts (current chart first, the device's
  threshold drawn).
  **Minimum on-time** (decided 2026-10-27, "producing" for heated
  machines): a device with `min_on_s` set reads `on` only once current has
  held at or above its threshold for that long, judged in `snapshot` by
  `fleet::producing_state` over `device_uplinks_since` (the last
  `2 × min_on_s`, ≤ `MIN_ON_ROWS`); one reading can switch it off, never on.
  `off` / `unknown` and `min_on_s` 0 are the plain judgement; the cap is
  `MAX_MIN_ON_S` (3600, enforced on input and when judging). `/fleet` rows
  carry `min_on_s` when set; the history page's power timeline stays the
  instantaneous reading. The live table patch flips a badge to `off` on an
  event but leaves `on` to the next server render when `data-min-on` > 0.
- Per-device threshold: `PUT /devices/{id}/threshold` (admin mTLS) with
  `{"power_threshold_ma": 12000, "min_on_s": 120}`; the body replaces both
  parts, so a threshold alone clears the min-on and `{}`, `null` or an
  empty body clears both (`deny_unknown_fields`, so a misspelt key is 422,
  never a clear; `min_on_s` > 3600 is 422). `POST /devices` accepts both
  as optional fields. The device page has the form (`POST
  /ui/devices/{id}/threshold`, CSRF; two number boxes). SQLite adds the
  columns on open (`ALTER TABLE` guarded by `pragma_table_info`); DynamoDB
  stores them as `N` attributes in one `SET … REMOVE …` update.
  `scripts/sim-fleet.sh` sets per-profile values for the simulated
  machines (`threshold_for`, `min_on_for`: 120 s for extruder and molder).
- Live view: `/ui/live` (+ `/ui/live.json` for the polling fallback) shows
  the last 120 s at *sample* resolution as 8-bit register levels
  (`supervictor_common::levels`: 1..=255 spans a fixed range per register,
  0 = absent). `src/live.rs` expands a stored `levels`/`levels_ms` burst
  into timestamped samples (last sample = receive time) or quantises the
  scalar fields into one; the SSE event carries `t_ms`, `levels`,
  `levels_ms`. The page is canvas (every animation frame; the line head
  trails by one sample and interpolates) drawing each device in its
  resolved style (`LiveDevice.style`: class / colour / dash / marker); the register
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
  that mode. Unset → plain HTTP, identity from the proxy header
  (`x-ssl-client-subject-dn`, Caddy / nginx). `tests/test_tls.rs` is a real TLS end-to-end with certs minted by
  `scripts/gen_certs.sh` (openssl required).
- Tests use in-memory SQLite by default (no AWS in the default suite).
  The `dynamo` backend has its own contract suite, `tests/test_store_dynamo.rs`,
  against a real DynamoDB Local (`DYNAMODB_LOCAL_ENDPOINT`, see the file
  header); CI runs it in the `dynamo` job with a service container.
- Wire contract (routes, field names) is shared with the firmware via
  `supervictor-common` — change it there, and the compiler finds every
  callsite on both sides.
