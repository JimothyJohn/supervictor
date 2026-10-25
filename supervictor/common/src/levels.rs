//! 8-bit register levels: one byte per telemetry register.
//!
//! A *register* is one charted telemetry field (`current`, `temp_c`, …).
//! Each has a fixed full-scale range, and a *level* is that range mapped
//! onto `1..=255` — 254 steps, so one step is 0.39 % of full scale and the
//! worst-case rounding error is 0.2 %. Level `0` is reserved for "no
//! reading" (a wired board has no RSSI; a sensor that failed to answer),
//! because a byte has no other way to say absent.
//!
//! The point is bytes per datapoint. A keyed float on the wire costs
//! ~15 bytes (`"temp_c":23.456,`); a level costs one — so a device can
//! sample ten times between uplinks and ship the whole burst as a flat
//! `levels` array ([`fields::UPLINK_LEVELS`]) with [`COUNT`] bytes per
//! sample, oldest first, at [`fields::UPLINK_LEVELS_MS`] spacing. The
//! scalar fields keep carrying the latest sample at full precision.
//!
//! This table is the contract: the firmware quantises with it and the
//! endpoint dequantises with it, from the same crate, so the two cannot
//! drift. Changing a range re-scales every stored burst — append new
//! registers at the end instead, and never reorder.

use crate::fields;

/// Level value meaning "no reading for this register in this sample".
pub const ABSENT: u8 = 0;
/// Lowest level with a reading: the register's `lo` bound.
pub const MIN: u8 = 1;
/// Highest level: the register's `hi` bound.
pub const MAX: u8 = 255;
/// Steps between `MIN` and `MAX`.
const STEPS: f32 = (MAX - MIN) as f32;

/// One telemetry register and its fixed full-scale range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Register {
    /// Key in the uplink payload (the scalar twin of this register).
    pub key: &'static str,
    /// Human label for charts.
    pub label: &'static str,
    /// Unit of the dequantised value.
    pub unit: &'static str,
    /// Value at level [`MIN`].
    pub lo: f32,
    /// Value at level [`MAX`].
    pub hi: f32,
}

/// Every register, in wire order. Position is the contract.
pub const REGISTERS: [Register; 5] = [
    Register {
        key: fields::UPLINK_CURRENT,
        label: "current",
        unit: "mA",
        lo: 0.0,
        // 20 A: the full range of the SEN0211 clamp the dev kit ships with
        // (decided 2026-10-25; HARDWARE.md). One step is 78.7 mA.
        hi: 20_000.0,
    },
    Register {
        key: fields::UPLINK_TEMP_C,
        label: "temperature",
        unit: "°C",
        lo: -20.0,
        hi: 60.0,
    },
    Register {
        key: fields::UPLINK_HUMIDITY_PCT,
        label: "humidity",
        unit: "%RH",
        lo: 0.0,
        hi: 100.0,
    },
    Register {
        key: fields::UPLINK_RSSI_DBM,
        label: "wifi rssi",
        unit: "dBm",
        lo: -100.0,
        hi: -20.0,
    },
    Register {
        key: fields::UPLINK_HEAP_FREE,
        label: "free heap",
        unit: "bytes",
        lo: 0.0,
        hi: 524_288.0,
    },
];

/// Registers per sample — the stride of a flat `levels` array.
pub const COUNT: usize = REGISTERS.len();

/// One sample: a level per register, in [`REGISTERS`] order.
pub type Levels = [u8; COUNT];

impl Register {
    /// Value difference between adjacent levels.
    pub fn step(&self) -> f32 {
        (self.hi - self.lo) / STEPS
    }

    /// Map a reading onto `MIN..=MAX`, rounding to the nearest level and
    /// clamping outside the range. `NaN` is "no reading" ([`ABSENT`]).
    pub fn quantize(&self, value: f32) -> u8 {
        if value.is_nan() {
            return ABSENT;
        }
        let scaled = (value - self.lo) / (self.hi - self.lo) * STEPS;
        if scaled <= 0.0 {
            MIN
        } else if scaled >= STEPS {
            MAX
        } else {
            // `scaled + 0.5` is in (0.5, 254.5): the truncating cast rounds
            // to nearest and stays in 0..=254, so `+ MIN` cannot overflow.
            (scaled + 0.5) as u8 + MIN
        }
    }

    /// Centre value of a level; `None` for [`ABSENT`].
    pub fn dequantize(&self, level: u8) -> Option<f32> {
        if level == ABSENT {
            return None;
        }
        Some(self.lo + f32::from(level - MIN) * (self.hi - self.lo) / STEPS)
    }
}

/// Quantise one reading per register (`None` → [`ABSENT`]).
pub fn pack(values: [Option<f32>; COUNT]) -> Levels {
    let mut out = [ABSENT; COUNT];
    for (i, (register, value)) in REGISTERS.iter().zip(values).enumerate() {
        if let Some(v) = value {
            out[i] = register.quantize(v);
        }
    }
    out
}

/// Dequantise one sample.
pub fn unpack(levels: &Levels) -> [Option<f32>; COUNT] {
    let mut out = [None; COUNT];
    for (i, (register, level)) in REGISTERS.iter().zip(levels).enumerate() {
        out[i] = register.dequantize(*level);
    }
    out
}

/// Number of samples in a flat `levels` array, or `None` when the length
/// is zero or not a whole number of samples.
pub fn sample_count(flat: &[u8]) -> Option<usize> {
    if flat.is_empty() || !flat.len().is_multiple_of(COUNT) {
        None
    } else {
        Some(flat.len() / COUNT)
    }
}

/// The `i`-th sample of a flat array (bounds-checked).
pub fn sample(flat: &[u8], i: usize) -> Option<Levels> {
    let start = i.checked_mul(COUNT)?;
    let slice = flat.get(start..start.checked_add(COUNT)?)?;
    let mut out = [ABSENT; COUNT];
    out.copy_from_slice(slice);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance budget the format promises: the worst rounding error is
    /// half a step, well under 0.5 % of full scale.
    const TOLERANCE: f32 = 0.005;

    #[test]
    fn every_level_round_trips_exactly() {
        for r in &REGISTERS {
            for level in MIN..=MAX {
                let v = r.dequantize(level).unwrap();
                assert_eq!(r.quantize(v), level, "{}: level {level} -> {v}", r.key);
            }
            assert_eq!(r.dequantize(MIN), Some(r.lo));
            assert_eq!(r.dequantize(MAX), Some(r.hi));
            assert_eq!(r.dequantize(ABSENT), None);
        }
    }

    #[test]
    fn error_is_under_half_a_percent_of_full_scale_everywhere() {
        for r in &REGISTERS {
            let span = r.hi - r.lo;
            // 10 001 points across the range, including both ends.
            for i in 0..=10_000u32 {
                let v = r.lo + span * (i as f32 / 10_000.0);
                let back = r.dequantize(r.quantize(v)).unwrap();
                let err = (back - v).abs() / span;
                assert!(
                    err <= TOLERANCE,
                    "{}: {v} -> {back} ({:.3} % of span)",
                    r.key,
                    err * 100.0
                );
                assert!(err <= r.step() / span / 2.0 + 1e-6, "{}: {v}", r.key);
            }
        }
    }

    #[test]
    fn quantize_is_monotonic_and_clamps() {
        for r in &REGISTERS {
            let span = r.hi - r.lo;
            let mut last = MIN;
            for i in 0..=2_000u32 {
                let v = r.lo - span + 3.0 * span * (i as f32 / 2_000.0);
                let q = r.quantize(v);
                assert!(q >= last, "{}: {v} went down ({last} -> {q})", r.key);
                assert!(q >= MIN, "{}: never ABSENT for a number", r.key);
                last = q;
            }
            assert_eq!(r.quantize(r.lo - 1e9), MIN);
            assert_eq!(r.quantize(r.hi + 1e9), MAX);
            assert_eq!(r.quantize(f32::NEG_INFINITY), MIN);
            assert_eq!(r.quantize(f32::INFINITY), MAX);
            assert_eq!(r.quantize(f32::NAN), ABSENT);
        }
    }

    #[test]
    fn pack_and_unpack_keep_absence() {
        let packed = pack([Some(1_240.0), Some(23.5), None, Some(-67.0), Some(73_728.0)]);
        assert_eq!(packed[2], ABSENT);
        assert!(packed.iter().enumerate().all(|(i, l)| i == 2 || *l >= MIN));
        let values = unpack(&packed);
        assert_eq!(values[2], None);
        assert!((values[0].unwrap() - 1_240.0).abs() <= REGISTERS[0].step() / 2.0);
        assert!((values[3].unwrap() + 67.0).abs() <= REGISTERS[3].step() / 2.0);
    }

    #[test]
    fn flat_arrays_must_be_whole_samples() {
        assert_eq!(sample_count(&[]), None);
        assert_eq!(sample_count(&[1; COUNT - 1]), None);
        assert_eq!(sample_count(&[1; COUNT]), Some(1));
        assert_eq!(sample_count(&[1; COUNT * 7]), Some(7));
        assert_eq!(sample_count(&[1; COUNT * 7 + 1]), None);
        let flat: [u8; COUNT * 2] = core::array::from_fn(|i| i as u8);
        assert_eq!(sample(&flat, 0).unwrap()[0], 0);
        assert_eq!(sample(&flat, 1).unwrap()[0], COUNT as u8);
        assert_eq!(sample(&flat, 2), None);
        assert_eq!(sample(&flat, usize::MAX), None);
    }

    /// The ranges are the stored-burst contract: a change re-scales every
    /// burst already in a store, so it is a deliberate, documented decision
    /// (docs/TELEMETRY.md), never a drive-by edit.
    #[test]
    fn register_ranges_are_the_documented_contract() {
        let ranges: [(f32, f32); COUNT] =
            core::array::from_fn(|i| (REGISTERS[i].lo, REGISTERS[i].hi));
        assert_eq!(
            ranges,
            [
                (0.0, 20_000.0),
                (-20.0, 60.0),
                (0.0, 100.0),
                (-100.0, -20.0),
                (0.0, 524_288.0),
            ]
        );
        // 20 A over 254 steps: 78.7 mA per level.
        assert!((REGISTERS[0].step() - 78.74).abs() < 0.01);
    }

    #[test]
    fn register_keys_are_the_wire_field_names() {
        let keys: [&str; COUNT] = core::array::from_fn(|i| REGISTERS[i].key);
        assert_eq!(
            keys,
            ["current", "temp_c", "humidity_pct", "rssi_dbm", "heap_free"]
        );
        for r in &REGISTERS {
            assert!(r.lo < r.hi, "{}", r.key);
        }
    }
}
