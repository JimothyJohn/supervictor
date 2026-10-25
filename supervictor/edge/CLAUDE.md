# supervictor edge

## Commands
```
../../Quickstart                                 # Full pipeline from repo root (lint, build, test)
cargo test --target aarch64-apple-darwin         # Run tests (NOT the default ESP32 target)
cargo run --bin supervictor-embedded --features embedded   # Build + flash via espflash
cargo run --bin supervictor-desktop --features desktop     # Desktop mTLS test client
cargo clippy --all-targets --target aarch64-apple-darwin   # Lint (examples are feature-gated)
```
Embedded builds read SSID/HOST/CERT_PATH/... from the environment at compile
time — load them with `set -a; source ../../.env.dev` first (Quickstart does
this automatically). CERT_PATH is repo-root-relative.

## no_std Constraints
- Library code is `#![no_std]` — no `String`, `Vec`, `format!`, or `std::` imports
- Use `heapless::String<N>` (aliased as `HString<N>`) for strings
- Use `serde-json-core` for JSON serialization (not `serde_json`)
- Buffer sizes and capacities are defined in `src/config.rs` — check before changing message formats
- The wire twin `models/uplink.rs` carries an optional `levels` burst: `UPLINK_BURST`
  samples per interval, one byte per register, quantised with `supervictor_common::levels`
  (the one shared dependency). `uplink_tests::full_uplink_with_max_id_fits_the_request_buffers`
  pins the JSON (640) / request (768) buffers in `network/http.rs`.

## Key Patterns
- **Async runtime**: Embassy — use `embassy_time::Timer`, never `std::thread::sleep`
- **TLS**: `mbedtls-rs` (embedded), `rustls` (desktop)
