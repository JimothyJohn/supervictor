# Remove Superfluous Dependencies

## Context
The device crate pulls in several dependencies whose actual usage is minimal — a handful of call sites that can be replaced with small hand-written functions. Each level below is independent and deployable on its own. They're ordered simplest → hardest so you can stop wherever the effort/reward tradeoff stops making sense.

---

## Level 1: `embedded-io` + `embedded-io-async` (delete 4 lines)
**Risk: Very low**

These are listed as explicit optional deps but never imported anywhere in `src/`. They exist only because `esp-mbedtls` uses them internally — but that's a transitive dependency, not something we need to declare.

### Changes
- **`Cargo.toml`**: Remove the two `[dependencies]` entries and their two entries in the `embedded` feature list.

### Verification
```
cargo test --lib --target aarch64-apple-darwin
cargo check --features embedded --target riscv32imc-unknown-none-elf
```

---

## Level 2: `rustls-pemfile` (replace 2 functions with ~80 LOC)
**Risk: Low**

Only two functions are used in `desktop_main.rs`: `certs()` and `private_key()`. PEM is a trivial format (BEGIN/END markers + base64 content).

### Changes
- **New file `src/network/pem.rs`** (~80 lines, `#[cfg(feature = "desktop")]`):
  - `parse_certs(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, ...>` — find `BEGIN CERTIFICATE` markers, base64-decode DER
  - `parse_private_key(pem: &[u8]) -> Result<PrivateKeyDer<'static>, ...>` — find `BEGIN PRIVATE KEY` / `BEGIN RSA PRIVATE KEY` / `BEGIN EC PRIVATE KEY`, base64-decode, return correct `PrivateKeyDer` variant
  - Inline base64 decoder (~30 lines, standard alphabet)
  - Tests: known PEM → DER roundtrip, multi-cert chain, error on empty/truncated input
- **`src/network/mod.rs`**: Add `#[cfg(feature = "desktop")] pub mod pem;`
- **`src/bin/desktop_main.rs`**: Replace `use rustls_pemfile::{certs, private_key}` with calls to `supervictor::network::pem::{parse_certs, parse_private_key}`. Remove `BufReader` wrappers (no longer needed — new fns take `&[u8]` directly).
- **`Cargo.toml`**: Remove `rustls-pemfile` dep and its entry in `desktop` feature.

### Verification
```
cargo test --lib --target aarch64-apple-darwin --features desktop
HOST=supervictor.advin.io cargo run --bin supervictor-desktop --features desktop  # manual smoke test
```

---

## Level 3: `static_cell` (replace macro with ~5 lines)
**Risk: Low-medium**

Used in exactly one macro in `embedded_main.rs` (2 call sites). Both calls happen in `main()` before any tasks are spawned — single-threaded init on a single-core MCU.

### Changes
- **`src/bin/embedded_main.rs`**: Rewrite the `make_static!` macro to use raw `MaybeUninit`:
  ```rust
  macro_rules! make_static {
      ($t:ty, $val:expr) => {{
          static mut STORAGE: core::mem::MaybeUninit<$t> = core::mem::MaybeUninit::uninit();
          unsafe { STORAGE.write($val) }
      }};
  }
  ```
- **`Cargo.toml`**: Remove `static_cell` dep and its entry in `embedded` feature.

### Verification
```
cargo check --features embedded --target riscv32imc-unknown-none-elf
```
Full verification requires on-device flash test (WiFi init + stack allocation).

---

## Level 4: `tokio-rustls` (replace 1 type with ~80 LOC, or simplify to blocking I/O)
**Risk: Medium**

Only `TlsConnector` is used (2 call sites in `desktop_main.rs`). It wraps `rustls::ClientConnection` for async I/O over tokio.

### Approach: Blocking simplification
Since the desktop binary does one request per loop iteration then sleeps, async TLS adds complexity with no benefit. Replace tokio's async TLS with `rustls::StreamOwned` over `std::net::TcpStream` (blocking). Keep tokio only for `#[tokio::main]` and `tokio::time::sleep`.

### Changes
- **`src/bin/desktop_main.rs`**:
  - Remove `use tokio_rustls::TlsConnector`
  - Remove `use tokio::io::{AsyncReadExt, AsyncWriteExt}` and `use tokio::net::TcpStream`
  - `create_connector()` returns `Arc<rustls::ClientConfig>` instead of `TlsConnector`
  - In the loop: use `std::net::TcpStream::connect()` + `rustls::ClientConnection::new()` + `rustls::StreamOwned::new()` for sync read/write
  - Keep `tokio::time::sleep` for the loop delay and `#[tokio::main]` for DNS (or switch DNS to `std::net::ToSocketAddrs`)
- **`Cargo.toml`**: Remove `tokio-rustls` dep and its entry in `desktop` feature.

### Verification
```
HOST=supervictor.advin.io cargo run --bin supervictor-desktop --features desktop
```

---

## Level 5: `serde` + `serde-json-core` (replace derives + JSON ser/de with ~120 LOC)
**Risk: High**

Serde is used for `#[derive(Serialize, Deserialize)]` on 2 structs + `serde_json_core::{to_string, from_str}` for JSON encoding/decoding. The actual shapes are simple and fixed.

### Key insight
`LambdaResponse` is already deserialized by hand in `parse_response()` — serde is only used on it in tests. The only production serde path is **serializing `UplinkMessage` to JSON**.

### Changes
- **New file `src/json.rs`** (~120 lines, `#![no_std]`):
  - `UplinkMessage::to_json<const N: usize>() -> Result<HString<N>, ()>` — writes `{"id":"...","current":NNN}` using `push_str` on `HString`
  - `UplinkMessage::from_json(json: &str) -> Result<Self, ()>` — minimal key-value extractor for test deserialization
  - Helper: `write_i32<const N: usize>(out: &mut HString<N>, val: i32)` — int-to-string without alloc
  - Helper: `extract_string_value(json, key) -> Result<&str, ()>` — finds `"key":"value"` pattern
  - Helper: `extract_i32_value(json, key) -> Result<i32, ()>` — finds `"key":NNN` pattern
  - Tests for all helpers + roundtrip on `UplinkMessage`
- **`src/lib.rs`**: Add `pub mod json;`
- **`src/models/uplink.rs`**:
  - Remove `use serde::{Deserialize, Serialize}`
  - Remove `#[derive(Serialize, Deserialize)]` from both structs (keep `Debug`, `Clone`)
  - Remove all `#[serde(rename = ...)]` attributes from `LambdaResponse`
- **`src/network/http.rs`**:
  - Remove `use serde::Serialize`
  - Change `post_request<T: Serialize>(host, data, path)` → `post_request(host: &str, json_body: &str, path: Option<&str>)` — accept pre-serialized JSON, removing the serde dependency from HTTP formatting entirely
  - Remove the `serde_json_core::to_string` call inside `post_request`; caller is now responsible for serialization
- **`src/bin/desktop_main.rs`**:
  - `let json_body = message.to_json::<512>().unwrap_or(...)` replaces `serde_json_core::to_string`
  - `post_request(host, json_body.as_str(), path)` replaces `post_request(host, &json_body, path)`
  - Line 105: replace `serde_json_core::to_string::<_, 1024>(&response.body).unwrap()` with `response.body.as_str()`
- **`src/network/http.rs` inline tests**: Update `test_post_request_formatting` — serialize `UplinkMessage` first, pass `&str` to `post_request`
- **`Cargo.toml`**: Remove `serde` and `serde-json-core` deps. Remove `features = ["alloc"]` if `serde` is the only user.

### Files modified
| File | Change |
|---|---|
| `src/json.rs` | **New** — serializer, deserializer, helpers |
| `src/lib.rs` | Add `pub mod json` |
| `src/models/uplink.rs` | Remove serde derives + renames |
| `src/network/http.rs` | Change `post_request` signature, remove serde import |
| `src/bin/desktop_main.rs` | Use `to_json()`, pass `&str` to `post_request` |
| `Cargo.toml` | Remove `serde`, `serde-json-core` |

### Verification
```
cargo test --lib --target aarch64-apple-darwin
cargo check --features embedded --target riscv32imc-unknown-none-elf
HOST=supervictor.advin.io cargo run --bin supervictor-desktop --features desktop
```
