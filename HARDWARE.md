# Supervictor dev kit — hardware spec

A one-cable telemetry device you can build from the parts already on the
bench, flash with the firmware in this repo, and clamp onto a machine in an
afternoon. Optimised for part count, robustness and prototyping speed, in
that order. Performance and novelty are explicitly not goals.

Companion documents: `docs/TELEMETRY.md` (wire schema and register ranges),
`TESTRIG.md` (which board is on which port, gitignored), `TODO.md` P2 (the
firmware items this kit unblocks), `hardware/SOURCING.md` (where to buy
each part, US design/build shops, budget and schedule per step),
`hardware/bom-devkit.csv` (cart-ready BOM) and
`hardware/RFQ-prototype.md` (the package a design/build vendor needs).
Written 2026-10-25 against `dev` b80f32d.

## 1. The kit in one paragraph

An **Olimex ESP32-POE-ISO** powered and networked by a single PoE Ethernet
cable from the **TP-Link LS108GP**, a **DFRobot Gravity SEN0211 20 A AC
current clamp** on one header pin for the primary `current` reading, and
optionally a **Sensirion SHT40 breakout** on the UEXT I²C pins for
`temp_c` / `humidity_pct`. Three jumper wires for the clamp, four for the
SHT40, nothing soldered, no enclosure for v0. Firmware is the existing
`ethernet` build plus two sensor drivers (section 7).

## 2. Bill of materials

| # | Part | Qty | Status | Role |
|---|------|-----|--------|------|
| 1 | Olimex ESP32-POE-ISO (WROOM-32E or 32UE) | 1 | **Have** (4 on the rack: `poe-iso-eth-01`, `poe-iso-rack-01..03`) | MCU, PoE PD, Ethernet, 3.3 V / 5 V rails |
| 2 | DFRobot Gravity Analog AC Current Sensor SEN0211 (20 A probe + conditioning module) | 1 | **Have** (from the `uplink` project) | `current` (mA) |
| 3 | Female–female 2.54 mm jumper wires | 3 (clamp) + 4 (SHT40) | Have | Module → EXT1/EXT2/UEXT |
| 4 | Cat5e/6 patch cable | 1 | Have | Power + data |
| 5 | TP-Link LS108GP PoE switch | 1 per rack | **Have** (racked 2026-10-25) | 802.3af/at PSE, 8 ports |
| 6 | Sensirion SHT40 breakout, STEMMA QT/Qwiic (Adafruit 4885 or any SHT4x board) | 1 | Buy (~$6) when `temp_c` / `humidity_pct` should stop being simulated | I²C on UEXT |
| 7 | AC line splitter (1× / 10× loops) | 1 | Buy if the test machine is corded | Lets the clamp see one conductor of a plug-in load |

Not in the kit, on purpose: the ESP32-POE2 (section 4.2 says when to use it),
the ESP32-C3 and WROVER WiFi devkits (WiFi-only, no PoE, and the Xtensa WiFi
build has the known TLS-reconnect wedge), the Raspberry Pi 5 (`pi-edge-01`
runs the desktop client and stays a Linux reference, not a kit part), and
the ST25DV NFC tag (section 12).

## 3. Why these parts

**ESP32-POE-ISO over ESP32-POE2.** Four reasons, each one a removed failure
mode:

- 3000 VDC galvanic isolation. USB serial and PoE can be attached at the
  same time, so you can `espflash monitor` a board that is powered the way
  it will be deployed. The POE2 forbids this (no isolation), which turns
  every debugging session into a cable-sequencing ritual.
- No PSRAM pin remap. GPIO16/17 are free, the UEXT I²C is the standard
  GPIO13/GPIO16 pair, and the RMII clock is the documented GPIO17 path that
  `src/board/poe_iso.rs` has been hardware-verified on since 2026-07-15.
- 802.3af. Powers from any PoE switch or injector, including the
  cheap ones. The POE2 is class 4 (802.3at) and only pays off when you want
  its 12/24 V output.
- Inventory: four of the six rack boards are POE-ISO.

**SEN0211 over a bare split-core CT (SCT-013 class) or an I²C power
monitor (INA2xx).** It is the sensor the previous product (`uplink`) shipped
with, so it is on the bench and its behaviour is known. The probe has its
burden built in (0–1 V AC out), so an unplugged probe is not a shock hazard
the way an open bare CT is, and the module turns that into a filtered DC
level the ESP32 ADC can read with zero external parts. An INA2xx would need
the load current routed through a shunt, which is the opposite of
non-invasive. A bare CT would need a burden resistor, a bias divider, a
coupling cap and rectification math in firmware: five parts and a week.

**SHT40 over BME280 / DHT22.** One I²C transaction, CRC on the wire, a
`no_std` embedded-hal 1.0 driver already published (`sht4x` 0.2.0), and the
UEXT already has 2.2 kΩ pull-ups on both lines, so the breakout plugs in
with four wires and no passives. It is also the part `docs/TELEMETRY.md`
shortlisted.

**Wired only.** One cable is power, data and physical identity (switch
port). No credentials baked at compile time, no `rssi_dbm`, no WiFi stack in
the image, no 2.4 GHz coexistence question until the ESP-NOW mesh spike
(TODO.md P2) says otherwise.

## 4. Board reference

### 4.1 ESP32-POE-ISO pin budget

Source: Olimex `ESP32-POE-ISO-PINOUT.png` (GitHub, read 2026-10-25),
cross-checked against arduino-esp32 `variants/esp32-poe-iso/pins_arduino.h`
and the Rev L schematic in `~/github/uplink/Hardware`. GPIO numbers are
ESP32 GPIO numbers. "GPI" pins are input-only (34, 35, 36, 39).

**Taken by the board** (never touch):

| GPIO | Function | Note |
|------|----------|------|
| 0 | Boot strap | Free on WROOM variants, but a strap pin: leave it |
| 1, 3 | UART0 TX/RX | USB programming and `espflash monitor` |
| 12 | PHY power | `PHY_POWER_GPIO` in `src/board/poe_iso.rs` |
| 17 | RMII clock out | `RMII_CLK_OUT_GPIO` |
| 18, 23 | MDIO, MDC | SMI bus to the LAN8710A |
| 19, 21, 22, 25, 26, 27 | RMII data | TXD0, TX_EN, TXD1, RXD0, RXD1, CRS_DV |
| 34 | BUT1 | User button, 10 kΩ pull-up |
| 35 | BAT_M | LiPo voltage divider |
| 39 | PWR_SENSE | External supply detect |

**UEXT** (10-pin 2.54 mm boxed header; pin 1 is the key side):

| Pin | Signal | GPIO | Board pull-up |
|-----|--------|------|---------------|
| 1 | +3.3 V | — | |
| 2 | GND | — | |
| 3 | UART TXD | 4 | |
| 4 | UART RXD | 36 (input-only) | 2.2 kΩ |
| 5 | I²C SCL | 16 | 2.2 kΩ |
| 6 | I²C SDA | 13 | 2.2 kΩ |
| 7 | SPI MISO | 15 | 10 kΩ, shared with SD CMD |
| 8 | SPI MOSI | 2 | 2.2 kΩ, shared with SD DATA0, boot strap |
| 9 | SPI SCK | 14 | shared with SD CLK |
| 10 | SPI SSEL | 5 | 10 kΩ, boot strap |

**EXT1** (left header, pin 1 at the +5 V end): 1 +5 V, 2 +3.3 V, 3 GND,
4 ESP_EN, 5 GPIO0, 6 GPIO1, 7 GPIO2, 8 GPIO3, 9 GPIO4, 10 GPIO5.

**EXT2** (right header, pin 1 at the GPI39 end): 1 GPI39, 2 GPI36,
3 GPI35, 4 GPI34, 5 GPIO33, 6 GPIO32, 7 GPIO16, 8 GPIO15, 9 GPIO14,
10 GPIO13. (arduino-esp32's "TX2 33 // ext2 pin 5" and "RX2 35 // ext2
pin 3" agree with this numbering.)

**Genuinely free for sensors with no strings attached:** GPIO32 and GPIO33
(both ADC1-capable, output-capable, no pull-ups), plus GPIO4 and GPIO13/16
on the UEXT. GPIO2/14/15 are free only while no microSD card is inserted.
Pins that appear on both UEXT and EXT headers are the same net: use one
connector or the other.

**Power out:** +5 V up to 0.4 A and +3.3 V up to 0.3 A, **2 W combined**,
while powered from PoE or USB. The PoE PD itself is 802.3af (12.95 W class),
so the board's own regulator is the limit, not the switch.

### 4.2 ESP32-POE2 differences (when you reach for it)

Use the POE2 when a sensor needs 12 V or 24 V or more than 2 W; otherwise
it is a worse dev board. Verified against the Olimex POE2 user manual
(GitHub, read 2026-10-25) and `src/board/poe2.rs`:

- No galvanic isolation. Never USB and Ethernet together; flash over USB,
  unplug, then cable.
- WROVER-E: GPIO16/17 belong to PSRAM. UEXT pin 5 (SCL) is **GPIO33**, the
  RMII clock leaves on **GPIO0** (`board-poe2` feature). Firmware constants
  must come from `board::active`, never hard-coded.
- 802.3at class 4, 25 W: header rails +5VP 1.5 A, VPP 12 V/1.5 A or
  24 V/0.75 A (PTH jumper `24V_E1`), +3.3 V 0.5 A.
- Same SMI/PHY-power pins (18/23/12), same BUT1 on GPI34, same BAT/PWR sense
  on GPI35/GPI39.

## 5. Wiring

### 5.1 Current clamp (required)

SEN0211 module 3-pin Gravity connector → POE-ISO headers:

| Module pin | Wire colour (typical Gravity lead) | POE-ISO | Why |
|------------|-----------------------|---------|-----|
| − (GND) | black | EXT1 pin 3 GND | |
| + (VCC) | red | EXT1 pin 2 **+3.3 V** | Module accepts 3.3–5.5 V. 3.3 V keeps the output pin-safe by construction and the useful range is ADC-limited anyway (section 6) |
| A (signal) | blue | EXT2 pin 6 **GPIO32** (ADC1_CH4) | ADC1 (ADC2 dies the moment the radio is on, which the mesh spike will want), no board pull-up, output-capable so it can be probed. Same pin the `uplink` firmware used |

Probe: plug the 3.5 mm jack into the module, then clamp around **one**
conductor of the machine's supply (L or N on single-phase, one leg on
three-phase). Around the whole cord the fields cancel and you read zero; on
a corded test load use the line splitter (BOM #7) or an extension cord with
one conductor separated. Keep the clamp jaws clean and fully closed; a
visible gap reads low.

Do **not** use GPI36 (UEXT RXD) for the clamp even though it is the ADC pin
the old `current-monitor` sketch used: on this board it carries a 2.2 kΩ
pull-up to 3.3 V, which biases the module output.

### 5.2 Temperature and humidity (optional)

SHT40 breakout → UEXT, four wires, no passives:

| Breakout | UEXT pin | GPIO |
|----------|----------|------|
| VIN / 3V | 1 | +3.3 V |
| GND | 2 | GND |
| SCL | 5 | 16 (POE2: 33) |
| SDA | 6 | 13 |

The board's 2.2 kΩ pull-ups in parallel with the breakout's 10 kΩ give
≈1.8 kΩ, fine at 100 kHz. I²C address 0x44. An Olimex UEXT ribbon or a
STEMMA QT pigtail with bare ends replaces the jumpers if you want it tidy.

### 5.3 Everything else

Nothing. No USB during normal operation (the POE-ISO tolerates it, the kit
does not need it). No LiPo. No microSD.

## 6. Electrical limits and conversions

### 6.1 Clamp signal chain

- Probe: 0–20 A RMS → 0–1 V AC, ±1 %, 50 Hz–1 kHz, 13 × 13 mm window.
- Module: 0–1 V AC in → 0.2–2.8 V DC out, ±4 %, 3.3–5.5 V supply. Inside
  (DFRobot schematic V1.0): a 3.0 V LDO (RT9193-30), an LMV321 at 2× gain,
  a 1N5819W peak detector into **10 µF with a 1 MΩ bleed**. That RC is a
  10 s time constant: when the load switches off the output decays, it does
  not drop. Expect ~30 s (3τ) before a reading falls below the floor.
- DFRobot's reference conversion (the one in `uplink/lib/uplink/src/uplink.h`),
  rewritten in millivolts and milliamps:

  ```text
  current_mA = adc_mV × 0.707 / 2 × 20 A/V  =  adc_mV × 7.07
  ```

  (0.707 undoes the module's peak detector, /2 its 2× gain, ×20 the probe
  range.) Sanity check: 2.8 V → 19.8 A, matching the datasheet span.

### 6.2 ESP32 ADC reality

ESP-IDF v4.4 ADC guide (ESP32 classic), 11 dB attenuation: **suggested
measurable range 150–2450 mV**; readings are nonlinear outside it and the
per-chip reference voltage varies. Consequences for this kit:

| Quantity | Value | Meaning |
|----------|-------|---------|
| ADC floor | 150 mV → **≈1.06 A** | Below about 1 A the reading is noise-shaped, not zero |
| ADC linear top | 2450 mV → **≈17.3 A** | Above it readings compress; 20 A is not reachable linearly |
| `current` register full scale | 6000 mA → 849 mV | Comfortably inside the linear window |
| `POWER_THRESHOLD_MA` default | 100 mA → 14 mV | **Unreadable** with this sensor; see section 11 |
| 12-bit count at 11 dB | ≈0.8 mV ≈ **5.4 mA** | Resolution is not the problem, accuracy is |
| Stack accuracy | ±1 % probe, ±4 % module, ADC gain error | Budget ±5–6 % of reading after a one-point calibration |

Mitigations that cost no parts: sample on ADC1 only, average 64 reads per
sample slot (Espressif's own recommendation alongside an optional 100 nF at
the pin, which the module's output filter already approximates), treat
anything under the floor as 0 mA, and clamp the published value to the
register range. Calibrate once per board against a known load (a resistive
heater of known wattage, or a clamp meter on the same conductor) and store
the gain in NVS or bake it at flash time like `DEVICE_NAME`.

### 6.3 Power budget

| Consumer | Draw | Source |
|----------|------|--------|
| POE-ISO board (ESP32 + PHY, link up) | ~1–2 W | TODO.md switch-day note; LS108GP shows per-port draw |
| SEN0211 module | not specified on the wiki; op-amp class, single-digit mA | measure on first bring-up |
| SHT40 | < 1 mA | datasheet class |
| **Headroom** | 2 W external budget minus the above | plenty; a 12 V sensor is the first thing that would not fit |

LS108GP: 62 W total, 30 W per port, power managed by measured draw, Extend
Mode OFF (it drops ports 1–4 to 10 Mb/s and the EMAC wants 100). Six boards
at 2 W is a tenth of the switch.

## 7. Firmware hooks

Everything below is additive to the `ethernet` build; nothing on the wire
changes (`current` is already the required i32 mA field, `temp_c` and
`humidity_pct` are optional floats).

**Board constants** in `src/board/poe_iso.rs` and `src/board/poe2.rs`,
re-exported through `board::active` like the Ethernet pins already are:

| Constant | POE-ISO | POE2 |
|----------|---------|------|
| `CURRENT_ADC_GPIO` | 32 (ADC1_CH4) | 32 |
| `I2C_SDA_GPIO` | 13 | 13 |
| `I2C_SCL_GPIO` | 16 | 33 |

**Drivers.** esp-hal 1.0.0 (pinned, `unstable` already on in
`supervictor/edge/Cargo.toml`):

- ADC: `esp_hal::analog::adc::{Adc, AdcConfig, Attenuation}`; `enable_pin(pin, Attenuation::_11dB)` → `Adc::new(peripherals.ADC1, cfg)` → `read_oneshot` (returns a `u16` count). Behind the `unstable` feature. Whether esp-hal ships an ADC calibration scheme for the classic ESP32 is **not verified**; plan on the one-point bench calibration in 6.2 and treat eFuse calibration as a bonus if the `AdcCal*` types compile for `esp32`.
- I²C: `esp_hal::i2c::master::{I2c, Config}`; `I2c::new(peripherals.I2C0, Config::default())?.with_sda(..).with_scl(..)`, stable in 1.0.0, implements embedded-hal 1.0 and embedded-hal-async (`into_async()`).
- SHT4x: crate `sht4x` 0.2.0 (embedded-hal 1.0, optional `embedded-hal-async`, `fixed`-point output, `no_std`). New dependency: name the trade-off in the PR per the dependency-review rule.

**Features.** `sensor-current` and `sensor-sht4x`, both off by default so CI
keeps building what it builds today; `scripts/flash.sh` grows a `FEATURES=`
row for the kit (`ethernet,sensor-current[,sensor-sht4x]`) in
`certs/rack-inventory.tsv`. When either sensor feature is on, the simulator
(`models/sim.rs`, `models/profile.rs`) must not fill that field: this is the
moment TODO.md P2 "simulator behind a cargo feature" lands as `sim`, on only
for `SIM_PROFILE` builds.

**Sampling.** The burst task already samples every 500 ms
(`UPLINK_BURST = 10` per 5 s uplink). Each slot: 64 ADC reads averaged,
converted with 6.1, floored per 6.2, quantised into the `current` register.
The SHT4x high-precision measurement takes ~10 ms; read it once per uplink,
not per slot, and skip NaN/CRC failures (the serializer turns non-finite
floats into `null`, which the endpoint rejects).

**Calibration knob.** `CURRENT_GAIN_PPM` compile-time env (default
1 000 000) applied after 6.1, baked like `UPLINK_INTERVAL_S`; per-board
value lives in a new column of `certs/rack-inventory.tsv` so `flash.sh`
carries it. NVS is the upgrade path once OTA exists and reflashing stops.

## 8. Bring-up checklist

Each step has a pass criterion; do not skip ahead on a fail.

1. **Board alone on PoE.** `./scripts/flash.sh <device> staging` over USB,
   cable it to the LS108GP. Pass: fleet row `fresh` within 60 s, `fw`
   matches, no `rssi_dbm`. (All six rack boards passed this 2026-10-25.)
2. **Clamp wired, no load.** Firmware with `sensor-current`, probe clamped
   around nothing. Pass: raw ADC < 150 mV equivalent and the published
   `current` is 0 mA (floor applied), stable for 5 minutes.
3. **Known resistive load.** Clamp one conductor of a heater/kettle of
   known wattage via the splitter. Pass: reading within ±10 % of P/V before
   calibration; set `CURRENT_GAIN_PPM` so it lands within ±3 %.
4. **Zero-return.** Switch the load off. Pass: `current` decays below the
   floor within ~30 s (the module's 10 s peak-hold, section 6.1) and the
   power badge flips (threshold per section 11).
5. **Inductive load.** A motor (shop vac, drill press). Pass: steady-state
   value plausible against the nameplate; inrush visible in the levels
   burst and the register pins at 255 rather than wrapping.
6. **SHT40.** Breath on the sensor. Pass: `humidity_pct` rises and falls,
   `temp_c` within 1 °C of a reference thermometer at rest.
7. **Soak.** 24 h on the rack. Pass: `heap_free` flat, `seq` contiguous,
   no watchdog reboots (`uptime_s` monotonic).
8. **Fault.** Pull the Ethernet cable 60 s. Pass: fleet row stale → dark →
   fresh after replug with no USB touch (TODO.md P2, still outstanding).

## 9. Robustness notes

- **Watchdog.** RWDT at 300 s fed only on a successful uplink round-trip
  (`WATCHDOG_TIMEOUT_S`); a wedged stack reboots in under five minutes. Keep
  sensor reads off the critical path so a hung I²C device cannot starve the
  feed: bound every I²C transaction with a timeout and fall back to the
  "no reading" level (0) for that register.
- **Isolation.** The POE-ISO's 3000 VDC barrier is between the Ethernet
  side and everything on the headers. The clamp probe is itself isolated
  from the conductor it measures; nothing in this kit touches mains
  galvanically. The SHT40 and the module share the board ground only.
- **Boot straps.** GPIO0, 2, 5, 12, 15 affect boot mode. The kit uses none
  of them for sensors; keep it that way when adding parts.
- **USB and PoE together** is allowed on the POE-ISO and forbidden on the
  POE2. `rack-flash.sh` already enforces the POE2 order.
- **Serial monitoring** without rebooting the board:
  `espflash monitor --non-interactive --chip esp32 --after no-reset --before no-reset-no-sync`.
- **Identity by MAC, not port.** USB enumeration moves; the inventory TSV
  keys on MAC.

## 10. Network and rack

LS108GP port 8 → house router (DHCP 10.0.0.0/24), ports 1–6 boards, port 7
the Mac Studio. The switch is unmanaged: there is no per-port power cycling
from software, so a hard reset of a board is a cable pull. If remote power
cycling becomes a want, that is the one argument for a managed PoE switch,
not for a different board.

Which board sits on which port is not yet recorded (`switch_port` column in
`certs/rack-inventory.tsv` is `-`); fill it on the next bench visit so a
dark fleet row maps to a cable.

## 11. Open decisions for Nick

- **`current` register full scale (6 A) vs the 20 A probe.** With the
  SEN0211 the linear window is ≈1–17 A. Either raise the register to 20 A
  (step 78 mA; changes the stored-burst scale, do it before real bursts
  ship) or swap the probe for the 5 A SEN0287 on the same module for
  small loads. The 60 A sim profiles will pin at 255 either way.
- **`POWER_THRESHOLD_MA`.** 100 mA is below the sensor floor. Proposal:
  default 1000 mA for kit devices, settable per device
  (`power_threshold_ma` already exists on the device record).
- **Where calibration lives.** Compile-time `CURRENT_GAIN_PPM` (simple,
  matches how everything else is baked) vs NVS (survives OTA). Compile-time
  for v0 is my pick.
- **Buy the SHT40 now or keep `temp_c` simulated until after OTA?** Six
  dollars; I would order it now so the `sim` feature flip happens once.

## 12. Later, deliberately not now

- **OTA** (TODO.md P2): single `factory` slot in `partitions.csv` today;
  otadata + two ~1.9 MB slots on 4 MB flash, one more USB flash per board.
  Nothing in this kit changes when it lands.
- **ST25DV NFC tag** (Adafruit 4701, on the bench from `uplink`): I²C on the
  same UEXT bus (GPO on a free GPIO, LPD optional). Tap-to-provision is the
  use; it waits for the mesh identity decision.
- **IMU** (`accel_g` / `gyro_dps` / `mag_ut`): ICM-20948 on the same I²C
  bus per `docs/TELEMETRY.md`; also a UEXT plug-in, no new pins.
- **Enclosure.** An off-the-shelf ABS box, or a DIN-rail clip and a
  cable gland for the probe lead is the industrial version. Not before the
  sensor stack is final.
- **POE2 as the "powered sensor" variant** when something needs 12/24 V.

## 13. Sources

- Olimex ESP32-POE-ISO pinout image and product page (features, 3000 VDC isolation, 802.3af, 2 W external budget), read 2026-10-25.
- Olimex ESP32-POE2 user manual (GitHub `DOCUMENTS/ESP32-POE2-user-manual.pdf`): UEXT table, GPIO33/GPIO0 remap, power rails, USB warning.
- arduino-esp32 `variants/esp32-poe-iso/pins_arduino.h`: UEXT pin ↔ GPIO, EXT2 pin comments, ETH defines.
- Olimex ESP32-PoE-ISO Rev L KiCad schematic (`~/github/uplink/Hardware`): net labels.
- DFRobot wiki SEN0211: probe and module specifications; conversion formula from `~/github/uplink/lib/uplink/src/uplink.h`.
- ESP-IDF v4.4 ADC guide (ESP32): attenuation ranges, ADC2/WiFi exclusion, multisampling and bypass-cap advice.
- esp-hal 1.0.0 docs (`esp32` target): `analog::adc` (unstable), `i2c::master` (stable).
- crates.io `sht4x` 0.2.0 dependency list; Adafruit product 4885 (SHT40, STEMMA QT, 0x44).
- This repo: `docs/TELEMETRY.md`, `supervictor/common/src/levels.rs`, `supervictor/edge/src/board/*.rs`, `supervictor/edge/src/config.rs`, `certs/rack-inventory.tsv`, `TODO.md`.
