# esp-hal 1.0.0 Migration Plan

Upgrade from `esp-hal v1.0.0-beta.0` ecosystem to `esp-hal v1.0.0` stable (released 2025-10-30).

## Resources

| Topic | Link |
|-------|------|
| esp-hal 1.0 release notes | https://github.com/esp-rs/esp-hal/releases/tag/esp-hal-v1.0.0 |
| esp-hal RC0 release (migration details) | https://github.com/esp-rs/esp-hal/releases/tag/esp-hal-v1.0.0-rc.0 |
| esp-hal MIGRATING.md | https://github.com/esp-rs/esp-hal/blob/main/MIGRATING.md |
| Espressif blog post | https://developer.espressif.com/blog/2025/10/esp-hal-1/ |
| mbedtls-rs repo (replaces esp-mbedtls) | https://github.com/esp-rs/mbedtls-rs |
| mbedtls-rs ESP example Cargo.toml | https://github.com/esp-rs/mbedtls-rs/blob/main/examples/esp/Cargo.toml |
| espflash v4 changelog | https://github.com/esp-rs/espflash/releases |
| ESP32-C3 flash encryption reference | https://espressif.github.io/esp32-c3-book-en/chapter_13/13.3/13.3.7.html |

---

## Phase 0: Prep

- [ ] Branch: `git checkout -b refactor/esp-hal-1.0`
- [ ] Verify current code compiles and tests pass on beta.0
- [ ] Snapshot working `Cargo.lock` for rollback

---

## Phase 1: Cargo.toml Dependency Bump

Rename and bump all crates in `device/Cargo.toml`.

### Crate Renames

| Old Crate | New Crate | Notes |
|-----------|-----------|-------|
| `esp-hal-embassy` 0.7.0 | `esp-rtos` 0.2 | Embassy executor integration merged here |
| `esp-wifi` 0.13.0 | `esp-radio` 0.17 | WiFi/BLE/radio stack |
| `esp-mbedtls` (git pin) | `mbedtls-rs` 0.1.0 | Published crate, no more git dep |

### Version Bumps

| Crate | Current | Target |
|-------|---------|--------|
| `esp-hal` | =1.0.0-beta.0 | 1 |
| `esp-alloc` | 0.7.0 | 0.9 |
| `esp-backtrace` | 0.15.1 | 0.18.1 |
| `esp-println` | 0.13.0 | 0.16 |
| `embassy-executor` | 0.7.0 | 0.9 |
| `embassy-time` | 0.4.0 | 0.5 |
| `embassy-net` | 0.6.0 | 0.8 |
| `embedded-io` | 0.6.1 | 0.7 |
| `embedded-io-async` | 0.6.1 | 0.7 |

### New Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `esp-bootloader-esp-idf` | 0.4 | Required by espflash v4 |
| `esp-metadata-generated` | 0.3 | Chip auto-detection for espflash v4 |

### Feature Flag Changes

| Crate | Old Features | New Features |
|-------|-------------|--------------|
| `esp-hal` | `defmt, esp32c3, unstable` | `log-04, esp32c3, unstable, exception-handler` |
| `esp-rtos` (was esp-hal-embassy) | `esp32c3` | `esp-radio, embassy` |
| `esp-radio` (was esp-wifi) | `builtin-scheduler, defmt, esp-alloc, esp32c3, wifi` | `wifi, log-04, unstable` |
| `esp-backtrace` | `defmt, esp32c3, exception-handler, panic-handler` | `esp32c3, panic-handler, println` |
| `esp-println` | `defmt-espflash, esp32c3, log` | `log-04` |
| `embassy-executor` | `defmt, task-arena-size-40960` | `task-arena-size-40960` |
| `mbedtls-rs` (was esp-mbedtls) | `esp32c3` | `accel-esp32c3` |

### Embedded Feature Set

```toml
# Before
embedded = [
  "esp-alloc", "esp-backtrace", "esp-hal", "esp-hal-embassy",
  "esp-println", "esp-wifi", "embassy-net", "embassy-executor",
  "embassy-time", "embedded-io", "embedded-io-async", "esp-mbedtls",
  "reqwless", "static_cell",
]

# After
embedded = [
  "esp-alloc", "esp-backtrace", "esp-hal", "esp-rtos",
  "esp-println", "esp-radio", "embassy-net", "embassy-executor",
  "embassy-time", "embedded-io", "embedded-io-async", "mbedtls-rs",
  "esp-bootloader-esp-idf", "esp-metadata-generated",
  "reqwless", "static_cell",
]
```

### Files Changed
- `device/Cargo.toml`

---

## Phase 2: espflash v4 + Bootloader

- [ ] Install espflash v4: `cargo install espflash`
- [ ] Add `esp_bootloader_esp_idf::esp_app_desc!()` macro to `embedded_main.rs`
- [ ] Update `.cargo/config.toml` runner (espflash v4 auto-detects chip/log-format from metadata, may simplify flags)
- [ ] Verify `espflash` config migration (v3 → v4 is automatic)

### Files Changed
- `device/src/bin/embedded_main.rs`
- `device/.cargo/config.toml`

---

## Phase 3: esp-hal API Migration

Changes required by the beta.0 → 1.0.0 API evolution.

### 3a. Peripheral Singletons (`embedded_main.rs`)

```diff
-let timg0 = TimerGroup::new(peripherals.TIMG0);
+let timg0 = TimerGroup::new(peripherals.TIMG0.reborrow());
```

### 3b. RNG API (`embedded_main.rs`)

```diff
-let mut rng = Rng::new(peripherals.RNG);
+let rng = Rng::new();
```

Random seed generation may need adjustment — verify `rng.random()` still returns `u32`.

### 3c. Main Macro (`embedded_main.rs`)

```diff
-#[esp_hal_embassy::main]
+#[esp_rtos::main]
 async fn main(spawner: embassy_executor::Spawner) -> ! {
```

### 3d. Embassy Init (`embedded_main.rs`)

```diff
-esp_hal_embassy::init(systimer.alarm0);
+// esp-rtos handles this via the #[esp_rtos::main] macro
```

### Files Changed
- `device/src/bin/embedded_main.rs`

---

## Phase 4: esp-wifi → esp-radio Migration

WiFi initialization and control APIs changed significantly.

### 4a. Imports (`embedded_main.rs`, `tasks.rs`)

```diff
-use esp_wifi::{init, EspWifiController};
-use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState};
+use esp_radio::wifi::{ControllerConfig, StationConfig};
```

### 4b. WiFi Initialization (`embedded_main.rs`)

```diff
-let wifi_ctrl = init(timg0.timer0, rng, peripherals.RADIO_CLK)?;
-let esp_wifi_ctrl = &*make_static!(EspWifiController<'static>, wifi_ctrl);
-let (controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI)?;
+let station_config = Config::Station(
+    StationConfig::default()
+        .with_ssid(env!("SSID"))
+        .with_password(env!("PASSWORD").into()),
+);
+let (mut controller, interfaces) = esp_radio::wifi::new(
+    peripherals.WIFI,
+    ControllerConfig::default().with_operation_mode(station_config),
+)?;
```

### 4c. Network Stack (`embedded_main.rs`)

```diff
-let (stack, runner) = embassy_net::new(interfaces.sta, ...);
+let (stack, runner) = embassy_net::new(interfaces.station, ...);
```

### 4d. Connection Task (`tasks.rs`)

WiFi is now fully async — `start`, `stop`, `connect`, `disconnect` are gone. Connection is implicit via `set_config` and managed by the driver. The `connection()` task needs a full rewrite.

```diff
-controller.start_async().await.unwrap();
-controller.connect_async().await;
-esp_wifi::wifi::wifi_state() == WifiState::StaConnected
+// WiFi starts automatically; reconnection is driver-managed
+// Task may reduce to monitoring link state via stack.is_link_up()
```

### Files Changed
- `device/src/bin/embedded_main.rs`
- `device/src/app/tasks.rs`

---

## Phase 5: esp-mbedtls → mbedtls-rs Migration

### 5a. TLS Context (`embedded_main.rs`)

```diff
-use esp_mbedtls::Tls;
-let mut tls = Tls::new(peripherals.SHA)?;
-tls.set_debug(TLS_DEBUG_LEVEL);
+// mbedtls-rs handles TLS context differently — check mbedtls-rs examples
+// Hardware acceleration enabled via `accel-esp32c3` feature flag
```

### 5b. Certificate Loading (`tls.rs`)

```diff
-use esp_mbedtls::{Certificates, X509};
+use mbedtls_rs::{...};  // New certificate types TBD
```

The `Certificates` struct and `X509::pem()` API will change. Reference: https://github.com/esp-rs/mbedtls-rs/tree/main/examples

### 5c. TLS Session (`tasks.rs`)

```diff
-use esp_mbedtls::{asynch::Session, Mode, Tls, TlsVersion};
-let mut session = Session::new(&mut socket, Mode::Client { servername }, TlsVersion::Tls1_3, certs, tls.reference())?;
-session.connect().await?;
+// mbedtls-rs async API — reference the client example in the repo
```

### Files Changed
- `device/src/bin/embedded_main.rs`
- `device/src/network/tls.rs`
- `device/src/app/tasks.rs`

---

## Phase 6: Build & Test

- [ ] `cargo build --features embedded --target riscv32imc-unknown-none-elf`
- [ ] `cargo test --lib --target aarch64-apple-darwin`
- [ ] `cargo clippy --target aarch64-apple-darwin`
- [ ] Flash and test on hardware: `cargo run --bin supervictor-embedded --features embedded`
- [ ] Verify mTLS handshake completes against `supervictor.advin.io/hello`

---

## Risk Notes

- **reqwless** v0.13.0 may need a bump for `embedded-io 0.7` / `embassy-net 0.8` compatibility. Check before Phase 1.
- **static_cell** usage may change if `esp-rtos` manages statics differently.
- **WiFi reconnection logic** is the biggest behavioral change — the old explicit `connect_async()` loop becomes driver-managed. Test thoroughly.
- **mbedtls-rs** is v0.1.0 — API may be less stable than the old git-pinned esp-mbedtls. Pin the version.
