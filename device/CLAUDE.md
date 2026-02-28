# supervictor device

Embedded Rust firmware for ESP32-C3 with mTLS uplink to AWS Lambda.

## Dual-Target Build
- **Embedded**: `riscv32imc-unknown-none-elf` (default in `.cargo/config.toml`)
- **Desktop**: host triple (e.g. `aarch64-apple-darwin`) for local TLS testing

## Commands
```
cargo test --lib --target aarch64-apple-darwin   # Run tests (NOT the default ESP32 target)
cargo run --bin supervictor-embedded --features embedded   # Build + flash via espflash
cargo run --bin supervictor-desktop --features desktop     # Desktop mTLS test client
cargo clippy --target aarch64-apple-darwin       # Lint
```

## no_std Constraints
- Library code is `#![no_std]` — no `String`, `Vec`, `format!`, or `std::` imports
- Use `heapless::String<N>` (aliased as `HString<N>`) for strings
- Use `serde-json-core` for JSON serialization (not `serde_json`)
- Buffer sizes and capacities are defined in `src/config.rs` — check before changing message formats

## Key Patterns
- **Async runtime**: Embassy — use `embassy_time::Timer`, never `std::thread::sleep`
- **TLS**: `mbedtls-rs` (embedded), `rustls` (desktop)
- **Certs**: embedded at compile-time via `include_str!` in `src/network/tls.rs`
- **HTTP**: hand-rolled request/response in `src/network/http.rs` (no HTTP library in no_std)
- **Models**: `src/models/uplink.rs` — `UplinkMessage` and `LambdaResponse` with heapless fields

## Architecture
```
src/
  lib.rs              # Crate root, conditional module exports
  config.rs           # All constants (host, ports, buffer sizes, timeouts)
  error.rs            # HttpError enum (no_std Display)
  models/uplink.rs    # UplinkMessage, LambdaResponse
  network/http.rs     # GET/POST builders, response parser
  network/tls.rs      # Certificate loading (embedded only)
  app/tasks.rs        # Embassy tasks: WiFi, networking, main app loop
  bin/embedded_main.rs  # ESP32 entry point
  bin/desktop_main.rs   # Desktop entry point
```
