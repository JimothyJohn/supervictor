# Add Rust Device Integration Tests to `qs staging`

## Problem

The staging pipeline deploys to the dev stack and runs Python integration tests,
but never tests Rust device code against the live endpoint. Existing Rust
integration tests (`sam_local_roundtrip.rs`) use raw TCP and can't reach HTTPS.

## Solution

After staging deploys, run Rust HTTPS integration tests against the deployed dev
stack using `reqwest` + `tokio` as `[dev-dependencies]` (avoids triggering
`desktop` feature binary compilation).

## Files

| File | Action | Purpose |
|------|--------|---------|
| `device/tests/deployed_roundtrip.rs` | New | HTTPS integration tests (reqwest + tokio) |
| `device/Cargo.toml` | Edit | Add reqwest + tokio as `[dev-dependencies]` |
| `quickstart/rust.py` | New | Shared `host_target()` for dev.py and staging.py |
| `quickstart/commands/staging.py` | Edit | Wire cargo test step after Python integration tests |

## Tests

- `deployed_get_root` — GET `/` → 200, body has `"message"`
- `deployed_post_uplink` — POST `/` with JSON → 200, body has `"device_id"`
- `deployed_post_boundary_current` — POST with `i32::MAX` → 200
- `deployed_post_missing_body` — POST empty → 400

## Verification

```bash
# Compile check (offline)
cd device && cargo test --test deployed_roundtrip --target aarch64-apple-darwin --no-run

# Skip behavior (offline, no env var)
cargo test --test deployed_roundtrip \
  --target aarch64-apple-darwin

# Dry run
qs staging --dry-run --verbose

# Live (requires deployed dev stack)
qs staging --verbose
```

## Pipeline After Change

```
qs staging
  ├── Dev gate (unchanged)
  ├── sam build + deploy dev
  ├── Python integration tests vs deployed stack     ← existing
  ├── Rust device integration tests vs deployed stack ← NEW
  ├── mTLS verification vs prod endpoint             ← existing
  └── "Staging pipeline passed."
```
