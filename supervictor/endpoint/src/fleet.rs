//! Fleet health: staleness classification and machine-readable snapshots.
//!
//! Shared by the JSON API (`GET /fleet`, `GET /fleet/summary`), the dashboard
//! (feature `ui`), and the staleness watchdog — one definition of "healthy".

use serde::Serialize;

use crate::error::AppError;
use crate::store::DeviceStore;
use crate::time::{now_rfc3339, parse_rfc3339_unix};
use supervictor_common::status;

/// Seconds since last uplink below which a device is "fresh".
pub const FRESH_SECS: u64 = 15 * 60;
/// Seconds since last uplink below which a device is "stale" (beyond: "dark").
pub const DARK_SECS: u64 = 2 * 60 * 60;
/// Default current (mA) at or above which a device counts as powered on
/// (`POWER_THRESHOLD_MA`). The simulator's dropouts read 0 and its
/// quietest baseline is 200, so 100 splits them.
pub const DEFAULT_POWER_THRESHOLD_MA: i64 = 100;

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

    Ok(devices
        .into_iter()
        .map(|device| {
            let (last_uplink, fw, current) = match latest.get(&device.device_id) {
                Some((at, fw, current)) => (Some(at.clone()), fw.clone(), *current),
                None => (None, None, None),
            };
            let staleness = staleness_of(last_uplink.as_deref(), now_unix);
            let device_threshold = device.effective_threshold_ma(threshold_ma);
            let power = power_state(current, staleness, device_threshold);
            let fw_pending = device
                .desired_fw
                .as_deref()
                .is_some_and(|want| fw.as_deref() != Some(want));
            FleetDevice {
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
                fw_pending,
                desired_fw: device.desired_fw,
            }
        })
        .collect())
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
}
