//! Fleet health: staleness classification and machine-readable snapshots.
//!
//! Shared by the JSON API (`GET /fleet`, `GET /fleet/summary`), the dashboard
//! (feature `ui`), and the staleness watchdog — one definition of "healthy".

use serde::Serialize;

use crate::error::AppError;
use crate::store::DeviceStore;
use crate::time::{format_rfc3339, now_rfc3339, parse_rfc3339_unix};
use supervictor_common::status;

/// Seconds since last uplink below which a device is "fresh".
pub const FRESH_SECS: u64 = 15 * 60;
/// Seconds since last uplink below which a device is "stale" (beyond: "dark").
pub const DARK_SECS: u64 = 2 * 60 * 60;
/// Default current (mA) at or above which a device counts as powered on
/// (`POWER_THRESHOLD_MA`). 1000 mA is the SEN0211 clamp's ADC floor
/// (HARDWARE.md §6.2): below it a kit device reads noise, not a machine,
/// so the fleet default calls that "off" (decided 2026-10-26). Machines
/// that idle above it get a per-device `power_threshold_ma`; the
/// simulator profiles set theirs in `scripts/sim-fleet.sh`.
pub const DEFAULT_POWER_THRESHOLD_MA: i64 = 1000;
/// Longest minimum on-time a device may carry (an hour): bounds the
/// history a snapshot reads per device to judge "producing".
pub const MAX_MIN_ON_S: u32 = 3_600;
/// Uplink rows read per device for that judgement: `2 × MAX_MIN_ON_S` at
/// the 5 s uplink period with headroom. A device uplinking faster than
/// that within its window is judged on the newest rows the store returns,
/// which the oldest-first read makes the oldest ones — so the cap is set
/// well above anything the firmware sends.
pub const MIN_ON_ROWS: usize = 4_000;

/// How recently a device has been heard from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Staleness {
    /// Uplinked within the fresh window.
    Fresh,
    /// Uplinked, but not recently.
    Stale,
    /// No uplink for a long time.
    Dark,
    /// Never uplinked, or timestamp unparseable.
    Unknown,
}

impl Staleness {
    /// CSS class / short label.
    pub fn label(self) -> &'static str {
        match self {
            Staleness::Fresh => "fresh",
            Staleness::Stale => "stale",
            Staleness::Dark => "dark",
            Staleness::Unknown => "unknown",
        }
    }
}

/// Whether a device is drawing power, judged from its last `current`
/// reading against the fleet threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    /// Last current at or above the threshold.
    On,
    /// Last current below the threshold.
    Off,
    /// No current reading, or the last one is too old to trust.
    Unknown,
}

impl PowerState {
    /// CSS class / short label.
    pub fn label(self) -> &'static str {
        match self {
            PowerState::On => "on",
            PowerState::Off => "off",
            PowerState::Unknown => "unknown",
        }
    }
}

/// Classify a current reading. A reading from a dark or never-heard
/// device says nothing about its power now, so those are `Unknown`;
/// otherwise `current >= threshold` is `On` (the threshold itself counts
/// as on).
pub fn power_state(current: Option<i64>, staleness: Staleness, threshold_ma: i64) -> PowerState {
    match (current, staleness) {
        (Some(_), Staleness::Dark) | (Some(_), Staleness::Unknown) | (None, _) => {
            PowerState::Unknown
        }
        (Some(c), _) if c >= threshold_ma => PowerState::On,
        (Some(_), _) => PowerState::Off,
    }
}

/// One stored reading as the min-on judgement needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Receive time, Unix seconds.
    pub at_unix: u64,
    /// `current` from the payload, if it parsed.
    pub current: Option<i64>,
}

/// "Producing" rather than merely powered (decided 2026-10-27 for the
/// heated machines): `instant` — the last reading against the threshold —
/// held at `Off` until current has been at or above the threshold for
/// `min_on_s` continuously. `readings` are the device's stored uplinks,
/// oldest first, reaching back at least `2 × min_on_s` before the newest;
/// the run is walked back from the newest reading to the last one below
/// the threshold (or the oldest reading when none is), and `On` needs
/// `newest − run start ≥ min_on_s`. A lone reading is a run of length
/// zero, so a machine is `off` for its first `min_on_s` after any gap: a
/// heater zone pulsing for ten seconds never qualifies, a screw running
/// for a shift does. `min_on_s == 0` is the plain judgement.
pub fn producing_state(
    instant: PowerState,
    min_on_s: u32,
    threshold_ma: i64,
    readings: &[Reading],
) -> PowerState {
    if instant != PowerState::On || min_on_s == 0 {
        return instant;
    }
    let Some(newest) = readings.last() else {
        return PowerState::Off;
    };
    let mut run_start = newest.at_unix;
    for r in readings.iter().rev() {
        match r.current {
            Some(c) if c >= threshold_ma => run_start = r.at_unix.min(run_start),
            _ => break,
        }
    }
    if newest.at_unix.saturating_sub(run_start) >= u64::from(min_on_s) {
        PowerState::On
    } else {
        PowerState::Off
    }
}

/// `current` out of a stored payload: integer as sent, or a float rounded
/// toward zero (bucket means and hand-written rows), never NaN/inf.
pub fn payload_current(payload: &serde_json::Value) -> Option<i64> {
    let v = payload.get("current")?;
    v.as_i64()
        .or_else(|| v.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))
}

/// Classify a last-uplink timestamp against the fixed thresholds.
pub fn staleness_of(last_uplink: Option<&str>, now_unix: u64) -> Staleness {
    match last_uplink.and_then(parse_rfc3339_unix) {
        Some(at) => {
            let age = now_unix.saturating_sub(at);
            if age < FRESH_SECS {
                Staleness::Fresh
            } else if age < DARK_SECS {
                Staleness::Stale
            } else {
                Staleness::Dark
            }
        }
        None => Staleness::Unknown,
    }
}

/// One device's health, as served by `GET /fleet`.
#[derive(Debug, Serialize)]
pub struct FleetDevice {
    /// Unique device identifier.
    pub device_id: String,
    /// Owner of the device.
    pub owner_id: String,
    /// Lifecycle status (`active` / `inactive`).
    pub status: String,
    /// Most recent uplink timestamp, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_uplink: Option<String>,
    /// Freshness classification of `last_uplink`.
    pub staleness: Staleness,
    /// Firmware version last reported by the device, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fw: Option<String>,
    /// Last reported current in mA, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<i64>,
    /// Power state from `current` against `threshold_ma`.
    pub power: PowerState,
    /// Current (mA) this device is judged against: its own override, else
    /// the fleet default.
    pub threshold_ma: i64,
    /// Whether `threshold_ma` is a per-device override.
    pub threshold_overridden: bool,
    /// Seconds at or above the threshold before `power` reads `on`, when
    /// the device carries a minimum on-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_on_s: Option<u32>,
    /// Firmware version an operator wants this device on (OTA target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_fw: Option<String>,
    /// True while `desired_fw` is set and differs from the reported `fw`:
    /// the rollout is still in flight for this device.
    pub fw_pending: bool,
}

/// Aggregate counts, as served by `GET /fleet/summary`.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct FleetSummary {
    /// Total registered devices.
    pub total: usize,
    /// Devices with `active` status.
    pub active: usize,
    /// Devices with `inactive` status.
    pub inactive: usize,
    /// Active devices heard from within the fresh window.
    pub fresh: usize,
    /// Active devices heard from, but not recently.
    pub stale: usize,
    /// Active devices silent past the dark threshold.
    pub dark: usize,
    /// Active devices that have never uplinked.
    pub never: usize,
    /// Active devices judged against their own `power_threshold_ma`
    /// rather than the fleet default.
    pub threshold_overrides: usize,
    /// Active devices whose last current is at or above the threshold.
    pub on: usize,
    /// Active devices whose last current is below the threshold.
    pub off: usize,
}

/// Snapshot of every device's health at `now` (Unix seconds), with power
/// judged against each device's own threshold, else `threshold_ma`.
pub fn snapshot(
    store: &dyn DeviceStore,
    now_unix: u64,
    threshold_ma: i64,
) -> Result<Vec<FleetDevice>, AppError> {
    let devices = store.list_devices()?;
    let latest: std::collections::HashMap<String, (String, Option<String>, Option<i64>)> = store
        .latest_uplinks()?
        .into_iter()
        .map(|u| {
            let fw = u
                .payload
                .get("fw")
                .and_then(|v| v.as_str())
                .map(String::from);
            let current = payload_current(&u.payload);
            (u.device_id, (u.received_at, fw, current))
        })
        .collect();

    let mut out = Vec::with_capacity(devices.len());
    for device in devices {
        let (last_uplink, fw, current) = match latest.get(&device.device_id) {
            Some((at, fw, current)) => (Some(at.clone()), fw.clone(), *current),
            None => (None, None, None),
        };
        let staleness = staleness_of(last_uplink.as_deref(), now_unix);
        let device_threshold = device.effective_threshold_ma(threshold_ma);
        let instant = power_state(current, staleness, device_threshold);
        let min_on_s = device.effective_min_on_s();
        // Only a device that reads `on` right now and carries a minimum
        // on-time needs its recent history; everything else is settled.
        let power = if instant == PowerState::On && min_on_s > 0 {
            let newest_unix = last_uplink
                .as_deref()
                .and_then(parse_rfc3339_unix)
                .unwrap_or(now_unix);
            let since = format_rfc3339(newest_unix.saturating_sub(2 * u64::from(min_on_s)), 0);
            let readings: Vec<Reading> = store
                .device_uplinks_since(&device.device_id, &since, MIN_ON_ROWS)?
                .iter()
                .map(|u| Reading {
                    at_unix: parse_rfc3339_unix(&u.received_at).unwrap_or(0),
                    current: payload_current(&u.payload),
                })
                .collect();
            producing_state(instant, min_on_s, device_threshold, &readings)
        } else {
            instant
        };
        let fw_pending = device
            .desired_fw
            .as_deref()
            .is_some_and(|want| fw.as_deref() != Some(want));
        out.push(FleetDevice {
            device_id: device.device_id,
            owner_id: device.owner_id,
            status: device.status,
            last_uplink,
            staleness,
            fw,
            current,
            power,
            threshold_ma: device_threshold,
            threshold_overridden: device.power_threshold_ma.is_some(),
            min_on_s: device.min_on_s.filter(|_| min_on_s > 0),
            fw_pending,
            desired_fw: device.desired_fw,
        });
    }
    Ok(out)
}

/// Aggregate a snapshot into summary counts. Staleness buckets count only
/// ACTIVE devices — a revoked device going quiet is expected, not an alert.
pub fn summarize(devices: &[FleetDevice]) -> FleetSummary {
    let mut summary = FleetSummary {
        total: devices.len(),
        ..FleetSummary::default()
    };
    for device in devices {
        if device.status == status::ACTIVE {
            summary.active += 1;
            match device.staleness {
                Staleness::Fresh => summary.fresh += 1,
                Staleness::Stale => summary.stale += 1,
                Staleness::Dark => summary.dark += 1,
                Staleness::Unknown => summary.never += 1,
            }
            match device.power {
                PowerState::On => summary.on += 1,
                PowerState::Off => summary.off += 1,
                PowerState::Unknown => {}
            }
            if device.threshold_overridden {
                summary.threshold_overrides += 1;
            }
        } else {
            summary.inactive += 1;
        }
    }
    summary
}

/// Convenience: snapshot at the current wall-clock time.
pub fn snapshot_now(
    store: &dyn DeviceStore,
    threshold_ma: i64,
) -> Result<Vec<FleetDevice>, AppError> {
    snapshot(
        store,
        parse_rfc3339_unix(&now_rfc3339()).unwrap_or(0),
        threshold_ma,
    )
}

#[cfg(test)]
mod power_tests {
    use super::*;

    #[test]
    fn threshold_itself_counts_as_on_and_stale_readings_still_count() {
        assert_eq!(
            power_state(Some(100), Staleness::Fresh, 100),
            PowerState::On
        );
        assert_eq!(
            power_state(Some(99), Staleness::Fresh, 100),
            PowerState::Off
        );
        assert_eq!(power_state(Some(0), Staleness::Fresh, 100), PowerState::Off);
        assert_eq!(
            power_state(Some(5_000), Staleness::Stale, 100),
            PowerState::On
        );
        assert_eq!(power_state(Some(-1), Staleness::Fresh, 0), PowerState::Off);
        assert_eq!(
            power_state(Some(i64::MAX), Staleness::Fresh, i64::MAX),
            PowerState::On
        );
    }

    #[test]
    fn dark_never_or_missing_readings_are_unknown() {
        assert_eq!(
            power_state(Some(900), Staleness::Dark, 100),
            PowerState::Unknown
        );
        assert_eq!(
            power_state(Some(900), Staleness::Unknown, 100),
            PowerState::Unknown
        );
        assert_eq!(
            power_state(None, Staleness::Fresh, 100),
            PowerState::Unknown
        );
    }

    #[test]
    fn payload_current_accepts_ints_and_finite_floats_only() {
        assert_eq!(
            payload_current(&serde_json::json!({"current": 1240})),
            Some(1240)
        );
        assert_eq!(
            payload_current(&serde_json::json!({"current": 1240.7})),
            Some(1240)
        );
        assert_eq!(
            payload_current(&serde_json::json!({"current": -3})),
            Some(-3)
        );
        assert_eq!(payload_current(&serde_json::json!({"current": "12"})), None);
        assert_eq!(payload_current(&serde_json::json!({"current": null})), None);
        assert_eq!(payload_current(&serde_json::json!({})), None);
        assert_eq!(payload_current(&serde_json::Value::Null), None);
    }

    fn r(at_unix: u64, current: i64) -> Reading {
        Reading {
            at_unix,
            current: Some(current),
        }
    }

    #[test]
    fn producing_passes_through_without_a_min_on_or_when_not_on() {
        let run = [r(0, 500), r(5, 500), r(10, 500)];
        assert_eq!(
            producing_state(PowerState::On, 0, 100, &run),
            PowerState::On
        );
        assert_eq!(
            producing_state(PowerState::Off, 60, 100, &run),
            PowerState::Off
        );
        assert_eq!(
            producing_state(PowerState::Unknown, 60, 100, &run),
            PowerState::Unknown
        );
        // No history at all cannot prove a run: off, never on.
        assert_eq!(
            producing_state(PowerState::On, 60, 100, &[]),
            PowerState::Off
        );
    }

    #[test]
    fn producing_needs_the_full_window_above_the_threshold() {
        // A heater zone pulse: three readings above, the one before below.
        let pulse = [r(100, 0), r(105, 24_000), r(110, 24_000), r(115, 24_000)];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &pulse),
            PowerState::Off
        );
        // The screw running: above for 60 s exactly, counted from the first
        // reading after the last dip.
        let run = [r(0, 0), r(5, 16_500), r(35, 16_500), r(65, 16_500)];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &run),
            PowerState::On
        );
        // One second short.
        let short = [r(0, 0), r(6, 16_500), r(35, 16_500), r(65, 16_500)];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &short),
            PowerState::Off
        );
        // A single reading is a run of length zero.
        assert_eq!(
            producing_state(PowerState::On, 1, 15_000, &[r(9, 99_999)]),
            PowerState::Off
        );
    }

    #[test]
    fn producing_run_starts_after_the_last_dip_not_the_window_start() {
        // Above for a long time, a one-reading dip, then above again for
        // 30 s: the dip restarts the clock.
        let dipped = [
            r(0, 20_000),
            r(500, 20_000),
            r(1_000, 0),
            r(1_010, 20_000),
            r(1_030, 20_000),
        ];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &dipped),
            PowerState::Off
        );
        assert_eq!(
            producing_state(PowerState::On, 20, 15_000, &dipped),
            PowerState::On
        );
        // The threshold itself counts as above, below it by one does not.
        let edge = [r(0, 14_999), r(10, 15_000), r(70, 15_000)];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &edge),
            PowerState::On
        );
        let edge = [r(0, 15_000), r(10, 14_999), r(70, 15_000)];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &edge),
            PowerState::Off
        );
    }

    #[test]
    fn producing_treats_an_unreadable_current_as_a_break_and_tolerates_disorder() {
        let gap = [
            r(0, 20_000),
            Reading {
                at_unix: 30,
                current: None,
            },
            r(40, 20_000),
            r(70, 20_000),
        ];
        assert_eq!(
            producing_state(PowerState::On, 60, 15_000, &gap),
            PowerState::Off
        );
        assert_eq!(
            producing_state(PowerState::On, 30, 15_000, &gap),
            PowerState::On
        );
        // Receive-time disorder inside the run (two rows in one second)
        // never makes the run look longer than its oldest member.
        let jumbled = [r(0, 0), r(50, 20_000), r(40, 20_000), r(70, 20_000)];
        assert_eq!(
            producing_state(PowerState::On, 31, 15_000, &jumbled),
            PowerState::Off
        );
        assert_eq!(
            producing_state(PowerState::On, 30, 15_000, &jumbled),
            PowerState::On
        );
        // Huge values never overflow the arithmetic.
        let huge = [r(0, i64::MAX), r(u64::MAX, i64::MAX)];
        assert_eq!(
            producing_state(PowerState::On, MAX_MIN_ON_S, i64::MAX, &huge),
            PowerState::On
        );
    }
}
