//! Settings the endpoint pushes to a device in the uplink response
//! (`config` block, decided 2026-11-02): the uplink interval and the power
//! threshold the dashboard judges it by. The block carries a `version`
//! derived from the values, so neither side keeps a counter: the device
//! applies and persists a block whose version differs from the one it
//! holds and echoes `config_version` in every uplink; the endpoint
//! compares that echo with the version of what it would send now and
//! reports `config_pending` until they match.

/// Shortest uplink interval a device accepts, in seconds.
pub const UPLINK_INTERVAL_MIN_S: u32 = 1;
/// Longest uplink interval a device accepts, in seconds. Past this the
/// fleet's staleness windows would read a healthy device as dark.
pub const UPLINK_INTERVAL_MAX_S: u32 = 600;

/// Interval setting meaning "the device's compiled default".
pub const UPLINK_INTERVAL_DEFAULT: u32 = 0;

/// Clamp a requested interval into what a device will run. `0` (the
/// compiled default) passes through unchanged.
pub const fn clamp_interval_s(requested: u32) -> u32 {
    if requested == UPLINK_INTERVAL_DEFAULT {
        UPLINK_INTERVAL_DEFAULT
    } else if requested < UPLINK_INTERVAL_MIN_S {
        UPLINK_INTERVAL_MIN_S
    } else if requested > UPLINK_INTERVAL_MAX_S {
        UPLINK_INTERVAL_MAX_S
    } else {
        requested
    }
}

/// The version of a config block: FNV-1a over the values, never `0`
/// (`0` is "no config applied" on the device). Equal values on both
/// sides give equal versions, which is the whole contract.
pub fn version(uplink_interval_s: u32, threshold_ma: i64) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in uplink_interval_s
        .to_le_bytes()
        .iter()
        .chain(threshold_ma.to_le_bytes().iter())
    {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    if h == 0 {
        1
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_a_pure_function_of_the_values_and_never_zero() {
        assert_eq!(version(5, 1000), version(5, 1000));
        assert_ne!(version(5, 1000), version(6, 1000));
        assert_ne!(version(5, 1000), version(5, 1001));
        assert_ne!(version(0, 0), 0);
        assert_ne!(
            version(0, 1000),
            version(5, 1000),
            "default vs explicit 5 s differ"
        );
        for i in [0u32, 1, 5, 600, u32::MAX] {
            for t in [i64::MIN, -1, 0, 1000, i64::MAX] {
                assert_ne!(version(i, t), 0, "{i} {t}");
            }
        }
    }

    #[test]
    fn interval_clamps_but_keeps_the_default() {
        assert_eq!(clamp_interval_s(0), 0);
        assert_eq!(clamp_interval_s(1), 1);
        assert_eq!(clamp_interval_s(5), 5);
        assert_eq!(clamp_interval_s(600), 600);
        assert_eq!(clamp_interval_s(601), 600);
        assert_eq!(clamp_interval_s(u32::MAX), 600);
    }
}
