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

Worst case (64-char id, every field at its widest printing) is ~475 bytes.
The firmware's buffers are sized for it: 512-byte JSON buffer and 1024-byte
request buffer in `supervictor/edge/src/network/http.rs`, guarded by
`uplink_tests::uplink_worst_case_fits_json_buffer`. Adding a field means
re-checking that test, not hoping.

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
