# Telemetry wire schema

The uplink packet (`POST /`) is the one message a device sends. Canonical
types: `supervictor/common/src/models.rs::UplinkMessage` (endpoint/CLI) and
its `no_std` twin `supervictor/edge/src/models/uplink.rs` — change both, plus
the field-name constants in `supervictor/common/src/fields.rs`.

Design rules:

- **Permanent facts don't ride the wire.** Owner, cert subject, registration
  time, and lifecycle status live in the `devices` reference table, keyed by
  `device_id`. The uplink carries only what changes between pings; resending
  static metadata every t seconds is redundant storage and bandwidth.
- **Everything past `id`/`current` is optional** and omitted (never `null`)
  when absent. Old firmware → new endpoint and new firmware → old endpoint
  both parse. The endpoint persists whatever arrives — new fields need no
  endpoint change.
- **Units live in field names** (`temp_c`, `gyro_dps`), so stored payloads
  are self-describing forever.

## Example packet (full telemetry)

```json
{
  "id": "factory-01",
  "current": 1240,
  "fw": "0.1.0",
  "seq": 17,
  "uptime_s": 86400,
  "interval_s": 300,
  "heap_free": 73728,
  "rssi_dbm": -67,
  "temp_c": 23.5,
  "humidity_pct": 41.2,
  "accel_g": [0.01, -0.02, 1.0],
  "gyro_dps": [0.5, -0.5, 0.0],
  "mag_ut": [22.1, -4.3, 40.0]
}
```

## Fields

### Identity + reading (required)

| Field | Type | Notes |
|-------|------|-------|
| `id` | string ≤64 | Device identity, from `DEVICE_NAME` at flash time |
| `current` | i32 | Primary sensor reading (mA) |

> Since 2026-10-24 the bench firmware and the desktop client fill `seq`,
> `uptime_s`, `interval_s`, `heap_free`, `rssi_dbm`, `temp_c` and
> `humidity_pct` on every ping. Until real sensors land the environmental
> values (and RSSI) come from `supervictor/edge/src/models/sim.rs`, a
> seeded simulator; `heap_free` and `uptime_s` are real on the ESP32.
>
> With a **machine profile** (`SIM_PROFILE`; `supervictor/edge/src/models/profile.rs`)
> the simulator impersonates plant equipment instead of the bench trace:
> `extruder`, `press-brake`, `cnc`, `saw`, `oven`, `compressor`, `conveyor`,
> `molder`, `welder`, `pump`. `current` is then the one-leg supply current
> in mA averaged over the uplink interval (tens of amps; motor inrush up to
> 6× rated for one second), following that machine's duty cycle on top of a
> working day of runs, breaks and shutdowns (shutdown = 0 mA, so the power
> state flips). `temp_c` becomes the machine's probe: barrel/oven
> temperature for the heated profiles (up to 400 °C), motor housing
> otherwise. Firmware takes `SIM_PROFILE` / `SIM_SPEED` at build time (an
> unknown name fails the build); the desktop client at runtime.
> `supervictor/edge/docker-compose.sim.yml` runs one container per profile
> (`./scripts/sim-fleet.sh up staging`).

### Device health (optional; firmware fills these every ping)

| Field | Type | Units | Why it's there |
|-------|------|-------|----------------|
| `fw` | string ≤16 | semver | Which devices need updating |
| `seq` | u32 | count | Reset → reboot; gap → dropped pings |
| `uptime_s` | u32 | seconds | Reboot forensics, watchdog correlation |
| `interval_s` | u32 | seconds | Device's own cadence (see below) |
| `heap_free` | u32 | bytes | Leak canary — should be flat over days |
| `rssi_dbm` | i8 | dBm | WiFi link quality; absent on wired Ethernet |

### Environment (optional; placeholders until sensors land)

| Field | Type | Units |
|-------|------|-------|
| `temp_c` | f32 | °C |
| `humidity_pct` | f32 | %RH |
| `accel_g` | [f32; 3] | g, [x, y, z] |
| `gyro_dps` | [f32; 3] | deg/s, [x, y, z] |
| `mag_ut` | [f32; 3] | µT, [x, y, z] |

Floats must be finite: the device serializer (`serde-json-core`) emits
non-finite floats as `null`, which the endpoint rejects inside a `[f32; 3]`.
When real sensor drivers land, clamp/skip NaN before filling these fields.

## Sub-interval burst: 8-bit levels (optional)

A *register* is one charted field — `current`, `temp_c`, `humidity_pct`,
`rssi_dbm`, `heap_free`, in that order. Each has a fixed full-scale range,
and a *level* maps that range onto one byte: `1..=255` is the range (254
steps, 0.39 % of full scale per step, 0.2 % worst rounding error) and `0`
means "no reading" (a wired board has no RSSI). The table lives once, in
`supervictor/common/src/levels.rs`, and both the firmware and the endpoint
quantise/dequantise with it — the edge crate depends on `supervictor-common`
for exactly this, so the two ends cannot drift.

| Register | `lo` (level 1) | `hi` (level 255) | step |
|----------|----------------|------------------|------|
| `current` (mA) | 0 | 6000 | 23.6 mA |
| `temp_c` | -20 | 60 | 0.31 °C |
| `humidity_pct` | 0 | 100 | 0.39 %RH |
| `rssi_dbm` | -100 | -20 | 0.31 dB |
| `heap_free` (bytes) | 0 | 524288 | 2 KiB |

The point is bytes per datapoint: a keyed float costs ~15 bytes on the
wire, a level costs one. So a device can sample ten times between uplinks
and ship the whole burst:

| Field | Type | Notes |
|-------|------|-------|
| `levels` | u8 array | Flat, 5 bytes per sample, oldest first; the last sample is the uplink instant. Length must be a multiple of 5, at most 600 samples |
| `levels_ms` | u32 | Milliseconds between consecutive samples; required with `levels`, `1..=3600000` |

The scalar fields keep carrying the latest sample at full precision, so a
burst-less uplink is unchanged and old firmware needs nothing. The endpoint
rejects a malformed burst (ragged length, missing or zero spacing, a byte
over 255) with 422 and stores nothing (`validate::check_levels`). Stored
bursts feed the live view (`/ui/live`, sample resolution, last 120 s) and
are folded into the history bucket means. Default firmware cadence:
`UPLINK_BURST = 10` samples over the 5 s interval → one every 500 ms
(`supervictor/edge/src/config.rs`); the desktop client takes `BURST=`.
Changing a register's range re-scales every stored burst — append new
registers at the end instead, never reorder.

## Uplink cadence

`interval_s` is the canonical knob: **t seconds between pings**, set per
build via the `UPLINK_INTERVAL_S` env var (default 5; zero or non-numeric
fails the build — `supervictor/edge/src/config.rs`). A user-facing
"frequency" option is just the inverse (f = 1/t) rendered at the UI layer;
the wire and config always carry the period. The device reports it in every
uplink so the cloud can judge staleness against each device's own cadence
instead of a fleet-wide guess.

## Retention

Uplink rows are not forever: the DynamoDB store stamps each row with
`expires_at = received_at + UPLINK_TTL_DAYS` (epoch seconds; default 30,
`0` disables) and the table's TTL reaps them. Long-lived history belongs in
a future rollup layer, not the raw ping table — this is the guardrail the
1.5M-row bench-pollution incident bought. Local SQLite ignores TTL.

## Payload budget

Worst case (64-char id, every field at its widest printing, the nine IMU
floats, a full 12-sample `levels` burst of three-digit bytes) is 660
bytes. The firmware's buffers are sized for it: 768-byte JSON buffer and
1024-byte request buffer in `supervictor/edge/src/network/http.rs`, guarded by
`uplink_tests::full_uplink_with_max_id_fits_the_request_buffers`. Adding a
field (or growing `LEVELS_SAMPLES_MAX`) means re-checking that test, not
hoping.

## Sensor candidates for the placeholders

All I²C, reachable via the POE-ISO's UEXT header. Shortlist to evaluate:

**9-axis IMU** (`accel_g` / `gyro_dps` / `mag_ut`)

- **TDK ICM-20948** — the standard 9-axis part since the MPU-9250 was
  discontinued; raw output, fusion is on you.
- **CEVA BNO085/BNO086** — 9-axis with on-chip sensor fusion (quaternion
  out); avoids writing fusion math in `no_std`, costs more.
- **Bosch BMI270 + BMM150** — 6-axis + 3-axis mag combo; very low power,
  two parts to drive.

**Temp / humidity** (`temp_c` / `humidity_pct`)

- **Sensirion SHT41/SHT45** — the accuracy benchmark for RH/T, trivial I²C.
- **Bosch BME280** — T/RH plus barometric pressure if a `pressure_hpa`
  field ever earns its slot.
- **TI HDC302x** — SHT4x-class alternative second source.

If device orientation/vibration is the actual goal, the BNO085's fused
output is worth the premium; if it's raw "is it moving / how is it mounted"
health data, the ICM-20948 is the default pick.
