//! Live view data: uplinks as timestamped 8-bit register samples.
//!
//! The live dashboard wants the newest two minutes at *sample* resolution,
//! not bucket means: a device that batches ten samples per uplink in a
//! `levels` burst (`supervictor_common::levels`) shows all ten, spaced
//! `levels_ms` apart ending at the receive time. A device without a burst
//! contributes one sample, quantised from its scalar fields, so old and
//! new firmware draw on the same strip. Pure functions over store rows;
//! `now` is injected.

use serde::Serialize;

use crate::error::AppError;
use crate::models::UplinkRecord;
use crate::store::DeviceStore;
use crate::time::{format_rfc3339, parse_rfc3339_unix_ms};
use supervictor_common::fields;
use supervictor_common::levels::{self, Levels, Register, COUNT, REGISTERS};

/// Seconds of history the live page is seeded with (and polls for).
pub const WINDOW_S: u64 = 120;
/// Most rows one live snapshot will read; beyond it the oldest part of
/// the window is dropped (the strip is a view, not a record).
pub const MAX_ROWS: usize = 20_000;

/// A validated sub-interval burst from one uplink payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burst {
    /// Flat levels, `COUNT` per sample, oldest first.
    pub levels: Vec<u8>,
    /// Milliseconds between consecutive samples.
    pub step_ms: u32,
}

impl Burst {
    /// Samples in the burst.
    pub fn len(&self) -> usize {
        self.levels.len() / COUNT
    }

    /// `true` when the burst holds no sample (never, for a parsed burst).
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Sample `i`, oldest first.
    pub fn sample(&self, i: usize) -> Option<Levels> {
        levels::sample(&self.levels, i)
    }

    /// Unix ms of sample `i` when the last sample landed at `received_ms`.
    pub fn time_of(&self, i: usize, received_ms: u64) -> u64 {
        let back = (self.len().saturating_sub(1).saturating_sub(i)) as u64;
        received_ms.saturating_sub(back.saturating_mul(u64::from(self.step_ms)))
    }
}

/// The burst carried by a stored payload, if it is well-formed: an array
/// of integers `0..=255` whose length is a whole number of samples, plus
/// a positive `levels_ms`. Anything else (a burst written before this
/// validation existed, or a hand-crafted row) is treated as absent — a
/// bad row degrades one device's strip, never the page.
pub fn burst_of(payload: &serde_json::Value) -> Option<Burst> {
    let raw = payload.get(fields::UPLINK_LEVELS)?.as_array()?;
    let step_ms = payload.get(fields::UPLINK_LEVELS_MS)?.as_u64()?;
    let step_ms = u32::try_from(step_ms).ok().filter(|ms| *ms > 0)?;
    let levels: Option<Vec<u8>> = raw
        .iter()
        .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect();
    let levels = levels?;
    levels::sample_count(&levels)?;
    Some(Burst { levels, step_ms })
}

/// Finite numeric value of each register's scalar field in `payload`.
pub fn scalar_values(payload: &serde_json::Value) -> [Option<f64>; COUNT] {
    let mut out = [None; COUNT];
    for (i, register) in REGISTERS.iter().enumerate() {
        out[i] = payload
            .get(register.key)
            .and_then(serde_json::Value::as_f64)
            .filter(|f| f.is_finite());
    }
    out
}

/// The scalar fields of `payload` quantised to one sample.
pub fn scalar_levels(payload: &serde_json::Value) -> Levels {
    let values = scalar_values(payload);
    levels::pack(values.map(|v| v.map(|f| f as f32)))
}

/// One timestamped sample on the live strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Sample {
    /// Unix milliseconds.
    pub t: u64,
    /// Level per register, `REGISTERS` order.
    pub l: Levels,
}

/// Every sample one uplink contributes: the burst, spaced `step_ms` apart
/// and ending at the receive time, or the quantised scalars as a single
/// sample. Rows whose timestamp does not parse contribute nothing.
pub fn samples_of(record: &UplinkRecord) -> Vec<Sample> {
    let Some(received_ms) = parse_rfc3339_unix_ms(&record.received_at) else {
        return Vec::new();
    };
    match burst_of(&record.payload) {
        Some(burst) => (0..burst.len())
            .filter_map(|i| {
                Some(Sample {
                    t: burst.time_of(i, received_ms),
                    l: burst.sample(i)?,
                })
            })
            .collect(),
        None => vec![Sample {
            t: received_ms,
            l: scalar_levels(&record.payload),
        }],
    }
}

/// Register description as the page script needs it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RegisterInfo {
    /// Payload key.
    pub key: &'static str,
    /// Chart title.
    pub label: &'static str,
    /// Unit of the dequantised value.
    pub unit: &'static str,
    /// Value at level 1.
    pub lo: f32,
    /// Value at level 255.
    pub hi: f32,
}

impl From<&Register> for RegisterInfo {
    fn from(r: &Register) -> Self {
        RegisterInfo {
            key: r.key,
            label: r.label,
            unit: r.unit,
            lo: r.lo,
            hi: r.hi,
        }
    }
}

/// One device's recent samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveDevice {
    /// Device identifier.
    pub device_id: String,
    /// How the device is drawn (same resolved style as the history views).
    pub style: crate::style::Resolved,
    /// Samples inside the window, oldest first.
    pub samples: Vec<Sample>,
}

impl LiveDevice {
    /// Newest sample, if any.
    pub fn latest(&self) -> Option<&Sample> {
        self.samples.last()
    }
}

/// `GET /ui/live.json` and the live page's seed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    /// Server time, Unix ms — the page aligns its clock to this.
    pub now_ms: u64,
    /// Seconds of history included.
    pub window_s: u64,
    /// Register table, wire order.
    pub registers: Vec<RegisterInfo>,
    /// Every registered device, sorted by id, silent ones with no samples.
    pub devices: Vec<LiveDevice>,
}

/// Assemble the live snapshot: every registered device's samples from the
/// last [`WINDOW_S`] seconds ending at `now_ms`.
pub fn snapshot(store: &dyn DeviceStore, now_ms: u64) -> Result<Snapshot, AppError> {
    let since_ms = now_ms.saturating_sub(WINDOW_S * 1_000);
    let registered = store.list_devices()?;
    let mut styles = crate::style::resolve(&registered);
    let mut ids: Vec<String> = registered.into_iter().map(|d| d.device_id).collect();
    ids.sort();

    // The oldest `MAX_ROWS` rows; a burst's earlier samples may predate
    // its row's receive time, so read one extra second back and filter.
    let since = format_rfc3339(since_ms / 1_000, 0);
    let rows = store.uplinks_since(&since, MAX_ROWS)?;

    let mut devices: Vec<LiveDevice> = ids
        .into_iter()
        .map(|device_id| LiveDevice {
            style: styles
                .remove(&device_id)
                .unwrap_or_else(|| crate::style::Resolved::slot(crate::style::SLOTS as usize)),
            device_id,
            samples: Vec::new(),
        })
        .collect();
    for row in &rows {
        let Some(device) = devices.iter_mut().find(|d| d.device_id == row.device_id) else {
            continue; // unregistered device: not on the dashboard
        };
        device
            .samples
            .extend(samples_of(row).into_iter().filter(|s| s.t >= since_ms));
    }
    for device in &mut devices {
        device.samples.sort_by_key(|s| s.t);
    }

    Ok(Snapshot {
        now_ms,
        window_s: WINDOW_S,
        registers: REGISTERS.iter().map(RegisterInfo::from).collect(),
        devices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::format_rfc3339;

    fn record(at_ms: u64, payload: serde_json::Value) -> UplinkRecord {
        UplinkRecord {
            device_id: "d".into(),
            received_at: format_rfc3339(at_ms / 1_000, (at_ms % 1_000) as u32 * 1_000_000),
            payload,
        }
    }

    #[test]
    fn burst_is_spaced_backwards_from_the_receive_time() {
        let flat: Vec<u8> = (1..=(COUNT * 3) as u8).collect();
        let rec = record(
            1_800_000_000_250,
            serde_json::json!({ "current": 5, "levels": flat, "levels_ms": 500 }),
        );
        let samples = samples_of(&rec);
        assert_eq!(samples.len(), 3);
        assert_eq!(
            samples[2].t, 1_800_000_000_250,
            "last sample = receive time"
        );
        assert_eq!(samples[1].t, 1_800_000_000_250 - 500);
        assert_eq!(samples[0].t, 1_800_000_000_250 - 1_000);
        assert_eq!(samples[0].l[0], 1);
        assert_eq!(samples[1].l[0], 1 + COUNT as u8);
        assert_eq!(samples[2].l[COUNT - 1], (COUNT * 3) as u8);
    }

    #[test]
    fn no_burst_means_one_quantised_scalar_sample() {
        let rec = record(
            1_800_000_000_999,
            serde_json::json!({ "current": 1240, "temp_c": 23.5, "rssi_dbm": -67 }),
        );
        let samples = samples_of(&rec);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].t, 1_800_000_000_999);
        let l = samples[0].l;
        assert_eq!(l[2], levels::ABSENT, "humidity absent");
        assert_eq!(l[4], levels::ABSENT, "heap absent");
        let back = REGISTERS[0].dequantize(l[0]).unwrap();
        assert!((back - 1240.0).abs() <= REGISTERS[0].step() / 2.0, "{back}");
        let back = REGISTERS[3].dequantize(l[3]).unwrap();
        assert!((back + 67.0).abs() <= REGISTERS[3].step() / 2.0, "{back}");
    }

    #[test]
    fn malformed_bursts_fall_back_to_scalars() {
        let one = vec![1u8; COUNT];
        for payload in [
            serde_json::json!({ "current": 1, "levels": [1, 2, 3], "levels_ms": 500 }),
            serde_json::json!({ "current": 1, "levels": one, "levels_ms": 0 }),
            serde_json::json!({ "current": 1, "levels": one }),
            serde_json::json!({ "current": 1, "levels": [256, 1, 1, 1, 1], "levels_ms": 5 }),
            serde_json::json!({ "current": 1, "levels": [-1, 1, 1, 1, 1], "levels_ms": 5 }),
            serde_json::json!({ "current": 1, "levels": [1.5, 1, 1, 1, 1], "levels_ms": 5 }),
            serde_json::json!({ "current": 1, "levels": "AQID", "levels_ms": 5 }),
            serde_json::json!({ "current": 1, "levels": [], "levels_ms": 5 }),
            serde_json::json!({ "current": 1, "levels": one, "levels_ms": 4_294_967_296u64 }),
            serde_json::json!({ "current": 1, "levels": one, "levels_ms": -5 }),
        ] {
            assert_eq!(burst_of(&payload), None, "{payload}");
            let samples = samples_of(&record(1_800_000_000_000, payload.clone()));
            assert_eq!(samples.len(), 1, "{payload}");
            assert!(samples[0].l[0] >= levels::MIN, "{payload}");
        }
        let bad_time = UplinkRecord {
            device_id: "d".into(),
            received_at: "yesterday".into(),
            payload: serde_json::json!({ "current": 1 }),
        };
        assert!(samples_of(&bad_time).is_empty());
    }

    #[test]
    fn hostile_scalars_are_absent_not_poison() {
        let payload = serde_json::json!({
            "current": "12", "temp_c": null, "humidity_pct": [1], "rssi_dbm": 1e308, "heap_free": -1e308
        });
        let values = scalar_values(&payload);
        assert_eq!(values[0], None);
        assert_eq!(values[1], None);
        assert_eq!(values[2], None);
        let l = scalar_levels(&payload);
        assert_eq!(l[3], levels::MAX, "huge clamps to the top");
        assert_eq!(l[4], levels::MIN, "huge negative clamps to the bottom");
    }

    #[test]
    fn burst_time_of_never_underflows() {
        let b = Burst {
            levels: vec![1; COUNT * 4],
            step_ms: u32::MAX,
        };
        assert_eq!(b.time_of(0, 10), 0);
        assert_eq!(b.time_of(3, 10), 10);
        assert_eq!(b.sample(4), None);
        assert!(!b.is_empty());
    }
}
