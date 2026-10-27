//! Seeded xorshift RNG and the per-board seed.
//!
//! Always compiled: the uplink loop jitters its period with it so boards
//! flashed together don't phase-lock, with or without the simulator
//! (feature `sim`), which draws its telemetry noise from the same source.

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

/// Jittered delay for the next uplink: `base_ms ± 20 %`, never below
/// 100 ms, so a fleet flashed at the same moment doesn't phase-lock on
/// the endpoint.
pub fn jitter_ms(rng: &mut Xorshift64, base_ms: u32) -> u32 {
    let span = base_ms / 5;
    let jitter = rng.range(-(span as i32), span as i32);
    base_ms.saturating_add_signed(jitter).max(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_within_twenty_percent_and_above_the_floor() {
        let mut rng = Xorshift64::new(7);
        for _ in 0..10_000 {
            let d = jitter_ms(&mut rng, 5_000);
            assert!((4_000..=6_000).contains(&d), "{d}");
        }
        for _ in 0..1_000 {
            assert!(jitter_ms(&mut rng, 10) >= 100);
        }
    }

    #[test]
    fn seed_depends_on_both_name_and_salt() {
        assert_ne!(seed_from("a", 0), seed_from("b", 0));
        assert_ne!(seed_from("a", 0), seed_from("a", 1));
        assert_eq!(seed_from("a", 1), seed_from("a", 1));
    }
}
