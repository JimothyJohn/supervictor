# Upgrade to esp-hal v1.0.0-rc.0 + espflash v4

## Why

The firmware pins `esp-hal = "=1.0.0-beta.0"`. The rc.0 release stabilizes APIs ahead of the 1.0 final, and is a prerequisite for espflash v4.3 which brings improved flash tooling, partition table handling, and defmt support.

Release notes: https://github.com/esp-rs/esp-hal/releases/tag/esp-hal-v1.0.0-rc.0

## Current Version Matrix

| Crate | Current | Location |
|---|---|---|
| `esp-hal` | `=1.0.0-beta.0` (pinned) | `Cargo.toml:58` |
| `esp-wifi` | `0.13.0` | `Cargo.toml:74` |
| `esp-hal-embassy` | `0.7.0` | `Cargo.toml:73` |
| `esp-alloc` | `0.7.0` | `Cargo.toml:51` |
| `esp-backtrace` | `0.15.1` | `Cargo.toml:52` |
| `esp-println` | `0.13.0` | `Cargo.toml:63` |
| `esp-mbedtls` | git rev `03458c3` | `Cargo.toml:85` |
| `embassy-net` | `0.6.0` | `Cargo.toml:40` |
| `embassy-executor` | `0.7.0` | `Cargo.toml:68` |
| `embassy-time` | `0.4.0` | `Cargo.toml:72` |
| `static_cell` | `2.1.0` | `Cargo.toml:82` |
| espflash (runner) | v3.x | `.cargo/config.toml:2` |

## Approach

### Step 1 — Identify the compatible version matrix

Check the [esp-hal v1.0.0-rc.0 release notes](https://github.com/esp-rs/esp-hal/releases/tag/esp-hal-v1.0.0-rc.0) for the companion crate versions. The esp-rs ecosystem releases all crates in lockstep. You need the exact versions of:

- `esp-hal`
- `esp-wifi`
- `esp-hal-embassy`
- `esp-alloc`
- `esp-backtrace`
- `esp-println`

Also check if `esp-mbedtls` has a compatible release or if the git rev needs updating. Search the [esp-mbedtls repo](https://github.com/esp-rs/esp-mbedtls) for commits after `03458c3` that reference `esp-hal 1.0.0-rc`.

### Step 2 — Update `Cargo.toml`

Remove the `=` pin on esp-hal and bump all esp-* crates to their rc.0-compatible versions:

```toml
# Before
esp-hal = { version = "=1.0.0-beta.0", ... }

# After (example — use actual version from release notes)
esp-hal = { version = "1.0.0-rc.0", ... }
```

Do NOT change feature flags yet — just versions. Run:

```bash
cargo check --features embedded --target riscv32imc-unknown-none-elf
```

Collect all compilation errors. They will reveal which APIs changed.

### Step 3 — Fix API breakage (file by file)

#### `src/bin/embedded_main.rs`

**Peripheral init** (line 41):
```rust
let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
```
Check if `Config` builder or `CpuClock::max()` changed.

**Timer setup** (lines 42-46):
```rust
let timg0 = TimerGroup::new(peripherals.TIMG0);
let systimer = SystemTimer::new(peripherals.SYSTIMER);
esp_hal_embassy::init(systimer.alarm0);
```
Check if `TimerGroup::new()` or `SystemTimer::new()` signatures changed. The `esp_hal_embassy::init()` function may now take a different argument.

**WiFi init** (lines 51-66):
```rust
let wifi_init_result = init(timg0.timer0, rng, peripherals.RADIO_CLK);
let (controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI)?;
```
This is the highest-risk area. The `esp_wifi::init()` and `esp_wifi::wifi::new()` signatures change frequently between releases. Look for:
- Different parameters (timer type, RNG type)
- Changed return type
- Renamed peripheral fields (e.g., `RADIO_CLK` → `RadioClk`)

**TLS context** (lines 78-84):
```rust
let mut tls = Tls::new(peripherals.SHA)?;
```
Check if `Tls::new()` still takes `SHA` peripheral directly.

**`#[esp_hal_embassy::main]`** (line 34):
Check if the proc macro attribute changed.

#### `src/app/tasks.rs`

**WiFi state machine** (lines 22-46):
```rust
esp_wifi::wifi::wifi_state() == WifiState::StaConnected
controller.wait_for_event(WifiEvent::StaDisconnected).await;
controller.set_configuration(&client_config).unwrap();
controller.start_async().await.unwrap();
controller.connect_async().await
```
Check: `wifi_state()` function, `WifiState`/`WifiEvent` enums, `WifiController` method names.

**TLS session** (lines 144-158):
```rust
Session::new(&mut socket, Mode::Client { servername: host_cstr }, TlsVersion::Tls1_3, certs, tls.reference())
```
Check `Session::new()` parameter order, `Mode::Client` struct fields, `tls.reference()` method.

**Runner type** (line 50):
```rust
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>)
```
Check if `WifiDevice` type path or generics changed.

#### `src/network/tls.rs`

```rust
X509::pem(ca_chain_bytes)
Certificates { ca_chain, certificate, private_key, password }
```
Check: `X509::pem()` return type, `Certificates` struct field names.

### Step 4 — Update espflash runner

Current runner in `.cargo/config.toml:2`:
```
espflash flash --monitor --chip esp32c3 --log-format defmt
```

Install espflash v4:
```bash
cargo install espflash@4
```

Test if the current CLI args still work. espflash v4 may:
- Rename `--chip` to `--target` or similar
- Change `--log-format` flag name
- Require an `espflash.toml` for configuration

If needed, create `device/espflash.toml`:
```toml
[flash]
chip = "esp32c3"
```

### Step 5 — Feature flag changes

Check the rc.0 release notes for renamed or removed features. Current features that may change:

- `esp-hal`: `"unstable"` feature — may be removed or renamed in rc.0
- `esp-wifi`: `"builtin-scheduler"` — check if still required
- `esp-println`: `"defmt-espflash"` — may change name with espflash v4
- `esp-hal-embassy`: feature list may change

### Step 6 — Verify embassy compatibility

The embassy crate versions (`embassy-net 0.6.0`, `embassy-executor 0.7.0`, `embassy-time 0.4.0`) must be compatible with the new `esp-hal-embassy`. Check the `esp-hal-embassy` Cargo.toml for its embassy version requirements.

## Testing

```bash
# 1. Compile check (embedded target)
cargo check --features embedded --target riscv32imc-unknown-none-elf

# 2. Clippy (embedded)
cargo clippy --features embedded --target riscv32imc-unknown-none-elf

# 3. Desktop library tests (no esp-hal dependency)
cargo test --lib --target aarch64-apple-darwin

# 4. Build binary (embedded)
cargo build --bin supervictor-embedded --features embedded

# 5. Flash and verify on device
cargo run --bin supervictor-embedded --features embedded
# Confirm: WiFi connects, TLS handshake succeeds, POST response received

# 6. Desktop client (uses rustls, not mbedtls — should be unaffected)
cargo run --bin supervictor-desktop --features desktop
```

## Risks

| Risk | Severity | Mitigation |
|---|---|---|
| esp-mbedtls git rev incompatible with rc.0 | HIGH | May need to find a newer rev or fork |
| WiFi init API completely restructured | HIGH | Follow migration guide in release notes |
| espflash v4 breaks `cargo run` workflow | MEDIUM | Fall back to `espflash flash` manually |
| Embassy version bump cascade | MEDIUM | Pin to versions specified by esp-hal-embassy |
| `"unstable"` feature removed | LOW | Remove from features list if APIs stabilized |

## Dependencies

None — this is independent of the other TODOs.
