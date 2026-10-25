//! Deterministic telemetry simulator for bench devices.
//!
//! Until real sensors land, every uplink carries plausible, *moving* values
//! so the fleet dashboard has something to show over time: a per-device
//! current baseline with slow drift, noise, occasional spikes and dropouts;
//! a mean-reverting temperature and humidity walk; RSSI jitter around a
//! per-device link quality; a slowly leaking heap. Everything is integer
//! arithmetic on a seeded xorshift generator — the same seed always yields
//! the same sequence, on the ESP32 and on the host, which is what makes it
//! testable. Hardware readings (real heap, real uptime) override the
//! simulated ones when the caller has them.

use heapless::String as HString;
use heapless::Vec as HVec;
use supervictor_common::levels;

use super::uplink::{UplinkMessage, LEVELS_CAP, LEVELS_SAMPLES_MAX};

/// Baseline current range a device settles around (mA).
const CURRENT_BASE_MIN: i32 = 200;
const CURRENT_BASE_MAX: i32 = 2_000;
/// Hard bounds on the emitted current.
pub const CURRENT_MIN: i32 = 0;
pub const CURRENT_MAX: i32 = 6_000;
/// Temperature walk bounds (milli-°C) and reversion target band.
pub const TEMP_MILLI_MIN: i32 = 10_000;
pub const TEMP_MILLI_MAX: i32 = 60_000;
const TEMP_TARGET_MIN: i32 = 20_000;
const TEMP_TARGET_MAX: i32 = 30_000;
/// Humidity walk bounds (deci-%RH).
pub const HUMIDITY_DECI_MIN: i32 = 100;
pub const HUMIDITY_DECI_MAX: i32 = 900;
const HUMIDITY_TARGET_MIN: i32 = 300;
const HUMIDITY_TARGET_MAX: i32 = 600;
/// RSSI bounds (dBm) and per-device base band.
pub const RSSI_MIN: i32 = -95;
pub const RSSI_MAX: i32 = -30;
const RSSI_BASE_MIN: i32 = -78;
const RSSI_BASE_MAX: i32 = -42;
/// Simulated heap when the caller has no real figure (bytes).
const HEAP_START: u32 = 96 * 1024;
pub const HEAP_FLOOR: u32 = 8 * 1024;
/// One sample in N is a spike; one in M a dropout to zero.
const SPIKE_ONE_IN: u32 = 40;
const DROPOUT_ONE_IN: u32 = 200;

/// xorshift64* — tiny, no_std, good enough for telemetry noise. Not a CSPRNG.
#[derive(Debug, Clone)]
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    /// A zero seed would lock xorshift at zero forever; remap it.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, n)`; `n == 0` yields 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        ((self.next_u64() >> 32) % u64::from(n)) as u32
    }

    /// Uniform integer in the closed range `[lo, hi]` (swapped bounds are tolerated).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
        lo.saturating_add(((self.next_u64() >> 32) % span) as i32)
    }
}

/// FNV-1a over the device id, so each board gets its own personality from
/// its name alone; `salt` (hardware RNG, boot count, ...) decorrelates runs.
pub fn seed_from(device_id: &str, salt: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in device_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h ^ salt.rotate_left(17)
}

/// Readings the firmware has for real; `None` means "simulate it".
#[derive(Debug, Clone, Copy, Default)]
pub struct Hardware {
    /// Free heap in bytes from the allocator.
    pub heap_free: Option<u32>,
    /// Link RSSI in dBm from the radio.
    pub rssi_dbm: Option<i8>,
}

/// One simulated reading set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Uplink counter since the simulator was created.
    pub seq: u32,
    /// Current in mA.
    pub current: i32,
    /// Temperature in °C.
    pub temp_c: f32,
    /// Relative humidity in %RH.
    pub humidity_pct: f32,
    /// RSSI in dBm.
    pub rssi_dbm: i8,
    /// Free heap in bytes.
    pub heap_free: u32,
}

impl Sample {
    /// The sample as one byte per register (`supervictor_common::levels`).
    pub fn levels(&self) -> levels::Levels {
        levels::pack([
            Some(self.current as f32),
            Some(self.temp_c),
            Some(self.humidity_pct),
            Some(f32::from(self.rssi_dbm)),
            Some(self.heap_free as f32),
        ])
    }
}

/// Stateful per-device simulator.
#[derive(Debug, Clone)]
pub struct Simulator {
    rng: Xorshift64,
    seq: u32,
    current_base: i32,
    current_drift: i32,
    temp_milli: i32,
    humidity_deci: i32,
    rssi_base: i32,
    heap: u32,
}

impl Simulator {
    /// Build from a seed (see [`seed_from`]).
    pub fn new(seed: u64) -> Self {
        let mut rng = Xorshift64::new(seed);
        let current_base = rng.range(CURRENT_BASE_MIN, CURRENT_BASE_MAX);
        let temp_milli = rng.range(TEMP_TARGET_MIN, TEMP_TARGET_MAX);
        let humidity_deci = rng.range(HUMIDITY_TARGET_MIN, HUMIDITY_TARGET_MAX);
        let rssi_base = rng.range(RSSI_BASE_MIN, RSSI_BASE_MAX);
        Self {
            rng,
            seq: 0,
            current_base,
            current_drift: 0,
            temp_milli,
            humidity_deci,
            rssi_base,
            heap: HEAP_START,
        }
    }

    /// Advance one tick, count it as an uplink and produce readings. Real
    /// `hardware` values win over simulated ones.
    pub fn sample(&mut self, hardware: Hardware) -> Sample {
        self.seq = self.seq.wrapping_add(1);
        self.tick(hardware)
    }

    /// Advance one tick without counting an uplink — a sub-interval sample
    /// inside a `levels` burst. Same walks, same rare events.
    pub fn tick(&mut self, hardware: Hardware) -> Sample {
        // Current: drift is a mean-reverting walk capped at ±25 % of base,
        // plus ±3 % noise, plus rare spikes (+50..150 %) and dropouts.
        let drift_cap = self.current_base / 4;
        let step = self.current_base / 50;
        self.current_drift = (self.current_drift + self.rng.range(-step, step)
            - self.current_drift / 8)
            .clamp(-drift_cap, drift_cap);
        let noise_span = self.current_base * 3 / 100;
        let mut current =
            self.current_base + self.current_drift + self.rng.range(-noise_span, noise_span);
        if self.rng.below(DROPOUT_ONE_IN) == 0 {
            current = 0;
        } else if self.rng.below(SPIKE_ONE_IN) == 0 {
            let pct = self.rng.range(50, 150);
            current = current.saturating_add(current / 100 * pct);
        }
        let current = current.clamp(CURRENT_MIN, CURRENT_MAX);

        // Temperature / humidity: random walk pulled back toward the band.
        self.temp_milli = walk(
            &mut self.rng,
            self.temp_milli,
            150,
            TEMP_TARGET_MIN,
            TEMP_TARGET_MAX,
            TEMP_MILLI_MIN,
            TEMP_MILLI_MAX,
        );
        self.humidity_deci = walk(
            &mut self.rng,
            self.humidity_deci,
            8,
            HUMIDITY_TARGET_MIN,
            HUMIDITY_TARGET_MAX,
            HUMIDITY_DECI_MIN,
            HUMIDITY_DECI_MAX,
        );

        let rssi_dbm = match hardware.rssi_dbm {
            Some(real) => real,
            None => (self.rssi_base + self.rng.range(-4, 4)).clamp(RSSI_MIN, RSSI_MAX) as i8,
        };

        let heap_free = match hardware.heap_free {
            Some(real) => real,
            None => {
                // Slow leak with jitter; a "reboot" restores it now and then.
                let leak = self.rng.below(48);
                let jitter = self.rng.below(1024);
                self.heap = self.heap.saturating_sub(leak).max(HEAP_FLOOR);
                if self.rng.below(2_000) == 0 {
                    self.heap = HEAP_START;
                }
                (self.heap + jitter).min(HEAP_START + 1024)
            }
        };

        Sample {
            seq: self.seq,
            current,
            temp_c: self.temp_milli as f32 / 1000.0,
            humidity_pct: self.humidity_deci as f32 / 10.0,
            rssi_dbm,
            heap_free,
        }
    }

    /// Jittered delay for the next uplink: `base_ms ± 20 %`, so a fleet
    /// flashed at the same moment doesn't phase-lock on the endpoint.
    pub fn next_delay_ms(&mut self, base_ms: u32) -> u32 {
        let span = base_ms / 5;
        let jitter = self.rng.range(-(span as i32), span as i32);
        base_ms.saturating_add_signed(jitter).max(100)
    }

    /// Build the wire message for one uplink interval: `burst` samples
    /// spread over `interval_s` (clamped to `1..=LEVELS_SAMPLES_MAX`). With
    /// `burst >= 2` the message carries every sample as 8-bit levels plus
    /// their spacing; the scalar fields are always the last sample at full
    /// precision, so a burst-less uplink (`burst == 1`) is unchanged.
    pub fn uplink(
        &mut self,
        id: HString<64>,
        uptime_s: u32,
        interval_s: u32,
        burst: usize,
        hardware: Hardware,
    ) -> UplinkMessage {
        let burst = burst.clamp(1, LEVELS_SAMPLES_MAX);
        let mut flat: HVec<u8, LEVELS_CAP> = HVec::new();
        for _ in 1..burst {
            let s = self.tick(hardware);
            // Capacity is LEVELS_SAMPLES_MAX samples; burst is clamped to it.
            let _ = flat.extend_from_slice(&s.levels());
        }
        let s = self.sample(hardware);
        let mut msg = UplinkMessage::new(id, s.current);
        msg.seq = Some(s.seq);
        msg.uptime_s = Some(uptime_s);
        msg.interval_s = Some(interval_s);
        msg.heap_free = Some(s.heap_free);
        msg.rssi_dbm = Some(s.rssi_dbm);
        msg.temp_c = Some(s.temp_c);
        msg.humidity_pct = Some(s.humidity_pct);
        if burst >= 2 {
            let _ = flat.extend_from_slice(&s.levels());
            msg.levels = flat;
            msg.levels_ms = Some((interval_s.saturating_mul(1_000) / burst as u32).max(1));
        }
        msg
    }
}

/// One step of a bounded, mean-reverting random walk. Outside the target
/// band the step is biased back toward it; the hard bounds always hold.
#[allow(clippy::too_many_arguments)]
fn walk(
    rng: &mut Xorshift64,
    value: i32,
    step: i32,
    target_lo: i32,
    target_hi: i32,
    hard_lo: i32,
    hard_hi: i32,
) -> i32 {
    let mut delta = rng.range(-step, step);
    if value < target_lo {
        delta += step / 2;
    } else if value > target_hi {
        delta -= step / 2;
    }
    value.saturating_add(delta).clamp(hard_lo, hard_hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNS: u32 = 20_000;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Simulator::new(seed_from("c3-wifi-01", 7));
        let mut b = Simulator::new(seed_from("c3-wifi-01", 7));
        for _ in 0..500 {
            assert_eq!(a.sample(Hardware::default()), b.sample(Hardware::default()));
        }
    }

    #[test]
    fn different_devices_diverge() {
        let mut a = Simulator::new(seed_from("c3-wifi-01", 0));
        let mut b = Simulator::new(seed_from("poe2-wifi-01", 0));
        let sa: [Sample; 8] = core::array::from_fn(|_| a.sample(Hardware::default()));
        let sb: [Sample; 8] = core::array::from_fn(|_| b.sample(Hardware::default()));
        assert_ne!(sa, sb);
    }

    #[test]
    fn salt_decorrelates_runs_of_the_same_device() {
        assert_ne!(seed_from("x", 1), seed_from("x", 2));
        let mut a = Simulator::new(seed_from("x", 1));
        let mut b = Simulator::new(seed_from("x", 2));
        let sa: [Sample; 8] = core::array::from_fn(|_| a.sample(Hardware::default()));
        let sb: [Sample; 8] = core::array::from_fn(|_| b.sample(Hardware::default()));
        assert_ne!(sa, sb);
    }

    #[test]
    fn zero_seed_is_not_stuck() {
        let mut rng = Xorshift64::new(0);
        let a = rng.next_u64();
        let b = rng.next_u64();
        assert_ne!(a, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn range_is_inclusive_and_tolerates_swapped_bounds() {
        let mut rng = Xorshift64::new(42);
        let mut seen_lo = false;
        let mut seen_hi = false;
        for _ in 0..RUNS {
            let v = rng.range(3, -2);
            assert!((-2..=3).contains(&v), "{v}");
            seen_lo |= v == -2;
            seen_hi |= v == 3;
        }
        assert!(seen_lo && seen_hi);
        assert_eq!(rng.range(5, 5), 5);
        assert_eq!(rng.below(0), 0);
        // Full i32 span must not overflow.
        let v = rng.range(i32::MIN, i32::MAX);
        let _ = v;
    }

    /// Every emitted value stays inside its documented bounds, for many
    /// seeds and many ticks — the dashboard's axes rely on this.
    #[test]
    fn samples_stay_in_bounds_across_seeds() {
        for seed in 0..64u64 {
            let mut sim = Simulator::new(seed.wrapping_mul(0x9E37_79B9) ^ 0xA5A5);
            let mut prev_seq = 0;
            for _ in 0..RUNS / 16 {
                let s = sim.sample(Hardware::default());
                assert!(s.seq == prev_seq + 1, "seq must count by one");
                prev_seq = s.seq;
                assert!((CURRENT_MIN..=CURRENT_MAX).contains(&s.current), "{s:?}");
                assert!((10.0..=60.0).contains(&s.temp_c), "{s:?}");
                assert!((10.0..=90.0).contains(&s.humidity_pct), "{s:?}");
                assert!(
                    (RSSI_MIN..=RSSI_MAX).contains(&i32::from(s.rssi_dbm)),
                    "{s:?}"
                );
                assert!(
                    s.heap_free >= HEAP_FLOOR && s.heap_free <= HEAP_START + 1024,
                    "{s:?}"
                );
            }
        }
    }

    /// The point of the exercise: values actually move, and the rare events
    /// (spikes, dropouts) happen at roughly their configured rates.
    #[test]
    fn current_varies_with_spikes_and_dropouts() {
        let mut sim = Simulator::new(seed_from("poe-iso-eth-01", 3));
        let base = sim.current_base;
        let mut distinct = 0;
        let mut last = -1;
        let mut dropouts = 0u32;
        let mut spikes = 0u32;
        for _ in 0..RUNS {
            let s = sim.sample(Hardware::default());
            if s.current != last {
                distinct += 1;
            }
            last = s.current;
            if s.current == 0 {
                dropouts += 1;
            } else if s.current > base * 14 / 10 {
                spikes += 1;
            }
        }
        assert!(
            distinct > RUNS / 2,
            "current barely moves: {distinct} changes"
        );
        let expect_drop = RUNS / DROPOUT_ONE_IN;
        assert!(
            dropouts > expect_drop / 2 && dropouts < expect_drop * 2,
            "dropouts {dropouts}, expected ~{expect_drop}"
        );
        let expect_spike = RUNS / SPIKE_ONE_IN;
        assert!(
            spikes > expect_spike / 3 && spikes < expect_spike * 2,
            "spikes {spikes}, expected ~{expect_spike}"
        );
    }

    #[test]
    fn hardware_readings_override_simulation() {
        let mut sim = Simulator::new(1);
        let s = sim.sample(Hardware {
            heap_free: Some(12_345),
            rssi_dbm: Some(-51),
        });
        assert_eq!(s.heap_free, 12_345);
        assert_eq!(s.rssi_dbm, -51);
    }

    #[test]
    fn next_delay_jitters_within_twenty_percent() {
        let mut sim = Simulator::new(9);
        let mut lo = u32::MAX;
        let mut hi = 0;
        for _ in 0..RUNS {
            let d = sim.next_delay_ms(5_000);
            assert!((4_000..=6_000).contains(&d), "{d}");
            lo = lo.min(d);
            hi = hi.max(d);
        }
        assert!(hi - lo > 1_000, "jitter too narrow: {lo}..{hi}");
        assert!(sim.next_delay_ms(0) >= 100, "never a zero delay");
    }

    #[test]
    fn uplink_carries_every_optional_field() {
        let mut sim = Simulator::new(seed_from("c3-wifi-01", 0));
        let msg = sim.uplink(
            "c3-wifi-01".try_into().unwrap(),
            3_600,
            5,
            1,
            Hardware::default(),
        );
        assert_eq!(msg.id.as_str(), "c3-wifi-01");
        assert_eq!(msg.seq, Some(1));
        assert_eq!(msg.uptime_s, Some(3_600));
        assert_eq!(msg.interval_s, Some(5));
        assert!(msg.heap_free.is_some());
        assert!(msg.rssi_dbm.is_some());
        assert!(msg.temp_c.is_some());
        assert!(msg.humidity_pct.is_some());
        assert!(msg.levels.is_empty(), "no burst for a single sample");
        assert_eq!(msg.levels_ms, None);
    }

    /// A burst is `burst` samples at `interval / burst` spacing, one byte
    /// per register, and its last sample is the scalar fields quantised;
    /// `seq` still counts uplinks, not samples.
    #[test]
    fn uplink_burst_packs_every_sample_and_ends_on_the_scalars() {
        let mut sim = Simulator::new(seed_from("c3-wifi-01", 0));
        let id: HString<64> = "c3-wifi-01".try_into().unwrap();
        let msg = sim.uplink(id.clone(), 10, 5, 10, Hardware::default());
        assert_eq!(msg.levels.len(), 10 * levels::COUNT);
        assert_eq!(msg.levels_ms, Some(500));
        assert_eq!(msg.seq, Some(1));
        let last = levels::sample(&msg.levels, 9).unwrap();
        assert_eq!(
            last,
            levels::pack([
                Some(msg.current as f32),
                msg.temp_c,
                msg.humidity_pct,
                msg.rssi_dbm.map(f32::from),
                msg.heap_free.map(|h| h as f32),
            ])
        );
        assert!(
            msg.levels.iter().all(|l| *l >= levels::MIN),
            "every register simulated"
        );
        // The samples move: not ten copies of one reading.
        let first = levels::sample(&msg.levels, 0).unwrap();
        assert_ne!(first, last);
        // Spacing follows the interval; burst is clamped to the capacity.
        let msg = sim.uplink(id.clone(), 20, 60, 100, Hardware::default());
        assert_eq!(msg.levels.len(), LEVELS_CAP);
        assert_eq!(msg.levels_ms, Some(60_000 / LEVELS_SAMPLES_MAX as u32));
        assert_eq!(msg.seq, Some(2));
        let msg = sim.uplink(id, 30, 1, 0, Hardware::default());
        assert!(msg.levels.is_empty(), "burst 0 behaves as 1");
        assert_eq!(msg.seq, Some(3));
    }

    #[test]
    fn hardware_readings_land_in_the_burst_too() {
        let mut sim = Simulator::new(3);
        let hw = Hardware {
            heap_free: Some(100 * 1024),
            rssi_dbm: Some(-40),
        };
        let msg = sim.uplink("x".try_into().unwrap(), 1, 2, 4, hw);
        for i in 0..4 {
            let s = levels::sample(&msg.levels, i).unwrap();
            assert_eq!(s[3], levels::REGISTERS[3].quantize(-40.0));
            assert_eq!(s[4], levels::REGISTERS[4].quantize(102_400.0));
        }
    }
}
