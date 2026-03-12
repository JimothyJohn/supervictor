# supervictor edge

## Commands
```
cargo test --lib --target aarch64-apple-darwin   # Run tests (NOT the default ESP32 target)
cargo run --bin supervictor-embedded --features embedded   # Build + flash via espflash 3.3.0
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
