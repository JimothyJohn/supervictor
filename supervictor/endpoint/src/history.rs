//! Fleet history: time-bucketed telemetry and a status timeline for the
//! dashboard's "over time" views (`GET /fleet/history`, `/ui/history`,
//! `/ui/devices/{id}`).
//!
//! Pure functions over store rows, with `now` injected, so the bucketing
//! and the staleness-over-time logic are testable without a clock. The
//! window is a fixed whitelist of presets (never a free-form duration from
//! the query string), the row count is capped, and the response says when
//! the cap cut the window short.

use std::collections::HashMap;

use serde::ser::SerializeMap;
use serde::Serialize;

use crate::error::AppError;
use crate::fleet::{staleness_of, Staleness, DARK_SECS};
use crate::live;
use crate::models::{DeviceRecord, UplinkRecord};
use crate::store::DeviceStore;
use crate::time::{format_rfc3339, parse_rfc3339_unix, parse_rfc3339_unix_ms};
use supervictor_common::levels;
use supervictor_common::status;

/// Buckets per window: every chart has the same x resolution, whatever
/// the window (15 m → 10 s buckets, 7 d → 1 h 52 m). 90 divides every
/// preset exactly, so `step * BUCKETS == seconds`.
pub const BUCKETS: usize = 90;
/// Most uplink rows one history request will read. Beyond it the window is
/// truncated at the recent end and `truncated` is set — a bounded read,
/// not a bounded dashboard; 50k is ~3 days of a 6-device fleet at 5 s.
pub const MAX_ROWS: usize = 50_000;

/// Window presets. Parsed from `?window=` by exact match only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// Last 15 minutes.
    M15,
    /// Last hour.
    H1,
    /// Last 6 hours.
    H6,
    /// Last 24 hours.
    H24,
    /// Last 7 days.
    D7,
}

impl Window {
    /// Every preset, in menu order.
    pub const ALL: [Window; 5] = [Window::M15, Window::H1, Window::H6, Window::H24, Window::D7];
    /// Default when the query carries no window.
    pub const DEFAULT: Window = Window::H1;

    /// Query-string token for the preset.
    pub fn label(self) -> &'static str {
        match self {
            Window::M15 => "15m",
            Window::H1 => "1h",
            Window::H6 => "6h",
            Window::H24 => "24h",
            Window::D7 => "7d",
        }
    }

    /// Window length in seconds.
    pub fn seconds(self) -> u64 {
        match self {
            Window::M15 => 15 * 60,
            Window::H1 => 60 * 60,
            Window::H6 => 6 * 60 * 60,
            Window::H24 => 24 * 60 * 60,
            Window::D7 => 7 * 24 * 60 * 60,
        }
    }

    /// Bucket width in seconds.
    pub fn step(self) -> u64 {
        (self.seconds() / BUCKETS as u64).max(1)
    }

    /// Whitelist parse: `None` or the empty token is the default, anything
    /// else must equal a preset label byte for byte (no trimming, no case
    /// folding). A near miss is an error for the caller to map to 4xx —
    /// never a default, so a typo cannot silently show the wrong range.
    pub fn parse(token: Option<&str>) -> Result<Window, AppError> {
        match token {
            None | Some("") => Ok(Window::DEFAULT),
            Some(t) => Window::ALL
                .into_iter()
                .find(|w| w.label() == t)
                .ok_or_else(|| AppError::InvalidPayload {
                    detail: "window must be one of 15m, 1h, 6h, 24h, 7d".into(),
                    structured: Some(serde_json::json!({
                        "field": "window",
                        "allowed": Window::ALL.iter().map(|w| w.label()).collect::<Vec<_>>(),
                    })),
                }),
        }
    }
}

/// The charted telemetry fields, in display order: the 8-bit register
/// table from `supervictor_common::levels` (`key`, `label`, `unit`, and
/// the full-scale range the live view and the `levels` bursts use).
pub use supervictor_common::levels::{Register as Metric, REGISTERS as METRICS};

/// Bucket means for one device: `values[i]` pairs with `METRICS[i]`;
/// `None` when no uplink in the bucket carried that field.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Bucket start, Unix seconds.
    pub t: u64,
    /// Uplinks that landed in the bucket.
    pub count: usize,
    /// Mean per metric.
    pub values: [Option<f64>; METRICS.len()],
}

impl Serialize for Point {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2 + METRICS.len()))?;
        m.serialize_entry("t", &self.t)?;
        m.serialize_entry("count", &self.count)?;
        for (metric, value) in METRICS.iter().zip(self.values.iter()) {
            m.serialize_entry(metric.key, value)?;
        }
        m.end()
    }
}

/// One device's bucketed series across the window.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeviceSeries {
    /// Device identifier.
    pub device_id: String,
    /// Uplinks inside the window (before bucketing).
    pub uplinks: usize,
    /// One entry per bucket, always `BUCKETS` long.
    pub points: Vec<Point>,
}

/// Active-device staleness counts as of the end of one bucket.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct TimelinePoint {
    /// Bucket start, Unix seconds (counts are evaluated at `t + step`).
    pub t: u64,
    /// Heard from within the fresh window.
    pub fresh: usize,
    /// Heard from, but not recently.
    pub stale: usize,
    /// Silent past the dark threshold.
    pub dark: usize,
    /// Never heard from as of that time.
    pub never: usize,
}

impl TimelinePoint {
    /// Active devices represented at this point.
    pub fn total(&self) -> usize {
        self.fresh + self.stale + self.dark + self.never
    }
}

/// The `/fleet/history` response.
#[derive(Debug, Clone, Serialize)]
pub struct History {
    /// Preset label (`"1h"`).
    pub window: &'static str,
    /// Window start, RFC 3339.
    pub since: String,
    /// Window end (`now`), RFC 3339.
    pub until: String,
    /// Bucket width in seconds.
    pub step_s: u64,
    /// Active-device staleness per bucket.
    pub timeline: Vec<TimelinePoint>,
    /// Per-device bucketed telemetry, sorted by device id, every
    /// registered device present (empty series for silent ones).
    pub devices: Vec<DeviceSeries>,
    /// Row cap hit: the recent end of the window is missing.
    pub truncated: bool,
}

/// Bucket index of `t` inside the closed window `[since, until]`, where
/// `until = since + BUCKETS * step` is "now". The end is inclusive because
/// `now` is whole seconds while `received_at` carries a fraction: an uplink
/// that landed this very second belongs in the last bucket, not nowhere.
fn bucket_of(t: u64, since: u64, step: u64) -> Option<usize> {
    let until = since + BUCKETS as u64 * step;
    if t < since || t > until {
        return None;
    }
    Some((((t - since) / step) as usize).min(BUCKETS - 1))
}

/// Bucket a device's rows (any order, any device mix — rows for other
/// devices are ignored) into `BUCKETS` mean points.
///
/// A row that carries a `levels` burst contributes every sample in it:
/// the earlier samples dequantised and placed `levels_ms` apart before the
/// receive time, the last one as the row's full-precision scalar fields.
/// `count` and `uplinks` still count rows, not samples.
pub fn bucket_series(
    device_id: &str,
    rows: &[UplinkRecord],
    since: u64,
    step: u64,
) -> DeviceSeries {
    let mut sums = vec![[0.0f64; METRICS.len()]; BUCKETS];
    let mut counts = vec![[0usize; METRICS.len()]; BUCKETS];
    let mut hits = vec![0usize; BUCKETS];
    let mut uplinks = 0;
    for row in rows.iter().filter(|r| r.device_id == device_id) {
        let Some(t) = parse_rfc3339_unix(&row.received_at) else {
            continue;
        };
        if let Some(burst) = live::burst_of(&row.payload) {
            let received_ms = parse_rfc3339_unix_ms(&row.received_at).unwrap_or(t * 1_000);
            for k in 0..burst.len().saturating_sub(1) {
                let Some(i) = bucket_of(burst.time_of(k, received_ms) / 1_000, since, step) else {
                    continue;
                };
                let Some(sample) = burst.sample(k) else {
                    continue;
                };
                for (m, v) in levels::unpack(&sample).iter().enumerate() {
                    if let Some(v) = v {
                        sums[i][m] += f64::from(*v);
                        counts[i][m] += 1;
                    }
                }
            }
        }
        let Some(i) = bucket_of(t, since, step) else {
            continue;
        };
        uplinks += 1;
        hits[i] += 1;
        for (m, metric) in METRICS.iter().enumerate() {
            if let Some(v) = row.payload.get(metric.key).and_then(as_finite_f64) {
                sums[i][m] += v;
                counts[i][m] += 1;
            }
        }
    }
    let points = (0..BUCKETS)
        .map(|i| {
            let mut values = [None; METRICS.len()];
            for m in 0..METRICS.len() {
                if counts[i][m] > 0 {
                    values[m] = Some(sums[i][m] / counts[i][m] as f64);
                }
            }
            Point {
                t: since + i as u64 * step,
                count: hits[i],
                values,
            }
        })
        .collect();
    DeviceSeries {
        device_id: device_id.to_string(),
        uplinks,
        points,
    }
}

/// Numbers only, finite only: a payload can carry anything, and a NaN
/// would poison a bucket mean and the chart's axis.
fn as_finite_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().filter(|f| f.is_finite())
}

/// Staleness counts of the active devices at the end of every bucket.
///
/// `rows` must cover at least `[since - DARK_SECS, now]` so a device's
/// last uplink before each bucket end is known; `latest` (device → last
/// uplink ever, Unix seconds) separates "dark" from "never" for devices
/// whose last uplink predates the rows.
pub fn status_timeline(
    devices: &[DeviceRecord],
    latest: &HashMap<String, u64>,
    rows: &[UplinkRecord],
    since: u64,
    step: u64,
) -> Vec<TimelinePoint> {
    // Per active device, ascending uplink times inside the rows.
    let mut times: HashMap<&str, Vec<u64>> = devices
        .iter()
        .filter(|d| d.status == status::ACTIVE)
        .map(|d| (d.device_id.as_str(), Vec::new()))
        .collect();
    for row in rows {
        if let (Some(list), Some(t)) = (
            times.get_mut(row.device_id.as_str()),
            parse_rfc3339_unix(&row.received_at),
        ) {
            list.push(t);
        }
    }
    for list in times.values_mut() {
        list.sort_unstable();
    }
    let mut cursors: HashMap<&str, usize> = times.keys().map(|k| (*k, 0)).collect();

    (0..BUCKETS)
        .map(|i| {
            let t0 = since + i as u64 * step;
            let at = t0 + step;
            let mut point = TimelinePoint {
                t: t0,
                fresh: 0,
                stale: 0,
                dark: 0,
                never: 0,
            };
            for (id, list) in &times {
                let cursor = cursors.get_mut(id).expect("cursor per device");
                while *cursor < list.len() && list[*cursor] <= at {
                    *cursor += 1;
                }
                let last_in_rows = cursor.checked_sub(1).map(|c| list[c]);
                let staleness = match last_in_rows {
                    Some(last) => staleness_of_unix(last, at),
                    None => match latest.get(*id) {
                        // Last uplink ever predates the rows → older than
                        // the lookback → dark by definition.
                        Some(&ever) if ever <= at => Staleness::Dark,
                        _ => Staleness::Unknown,
                    },
                };
                match staleness {
                    Staleness::Fresh => point.fresh += 1,
                    Staleness::Stale => point.stale += 1,
                    Staleness::Dark => point.dark += 1,
                    Staleness::Unknown => point.never += 1,
                }
            }
            point
        })
        .collect()
}

fn staleness_of_unix(last: u64, now: u64) -> Staleness {
    staleness_of(Some(&format_rfc3339(last, 0)), now)
}

/// Assemble the fleet history for `window` ending at `now_unix`.
pub fn fleet_history(
    store: &dyn DeviceStore,
    window: Window,
    now_unix: u64,
) -> Result<History, AppError> {
    let step = window.step();
    let since = now_unix.saturating_sub(step * BUCKETS as u64);
    let lookback = since.saturating_sub(DARK_SECS);

    let mut devices = store.list_devices()?;
    devices.sort_by(|a, b| a.device_id.cmp(&b.device_id));
    let latest: HashMap<String, u64> = store
        .latest_uplinks()?
        .into_iter()
        .filter_map(|u| parse_rfc3339_unix(&u.received_at).map(|t| (u.device_id, t)))
        .collect();

    let mut rows = store.uplinks_since(&format_rfc3339(lookback, 0), MAX_ROWS + 1)?;
    let truncated = rows.len() > MAX_ROWS;
    rows.truncate(MAX_ROWS);

    let timeline = status_timeline(&devices, &latest, &rows, since, step);
    let series = devices
        .iter()
        .map(|d| bucket_series(&d.device_id, &rows, since, step))
        .collect();

    Ok(History {
        window: window.label(),
        since: format_rfc3339(since, 0),
        until: format_rfc3339(now_unix, 0),
        step_s: step,
        timeline,
        devices: series,
        truncated,
    })
}

/// One device's bucketed series for `window` ending at `now_unix`, plus
/// whether the row cap truncated it.
pub fn device_history(
    store: &dyn DeviceStore,
    device_id: &str,
    window: Window,
    now_unix: u64,
) -> Result<(DeviceSeries, bool), AppError> {
    let step = window.step();
    let since = now_unix.saturating_sub(step * BUCKETS as u64);
    let mut rows =
        store.device_uplinks_since(device_id, &format_rfc3339(since, 0), MAX_ROWS + 1)?;
    let truncated = rows.len() > MAX_ROWS;
    rows.truncate(MAX_ROWS);
    Ok((bucket_series(device_id, &rows, since, step), truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::FRESH_SECS;

    const NOW: u64 = 1_800_000_000;

    fn device(id: &str, status: &str) -> DeviceRecord {
        DeviceRecord {
            device_id: id.into(),
            owner_id: "o".into(),
            subject_dn: None,
            status: status.into(),
            created_at: "2026-01-01T00:00:00+00:00".into(),
        }
    }

    fn row(id: &str, t: u64, payload: serde_json::Value) -> UplinkRecord {
        UplinkRecord {
            device_id: id.into(),
            received_at: format_rfc3339(t, 0),
            payload,
        }
    }

    #[test]
    fn window_parse_is_an_exact_whitelist() {
        assert_eq!(Window::parse(None).unwrap(), Window::H1);
        assert_eq!(Window::parse(Some("")).unwrap(), Window::H1);
        for w in Window::ALL {
            assert_eq!(Window::parse(Some(w.label())).unwrap(), w);
        }
        for bad in [
            " 6h ",
            "1H",
            "60m",
            "1h; DROP TABLE uplinks",
            "99999999999999999999d",
            "-1h",
            "1h\n",
            "１h",
            "1h\u{0}",
            "<script>",
            "all",
        ] {
            let err = Window::parse(Some(bad)).unwrap_err();
            assert!(
                matches!(err, AppError::InvalidPayload { .. }),
                "{bad:?} -> {err:?}"
            );
        }
    }

    #[test]
    fn window_steps_cover_the_window_exactly() {
        for w in Window::ALL {
            assert_eq!(w.step() * BUCKETS as u64, w.seconds(), "{w:?}");
        }
    }

    #[test]
    fn bucket_series_averages_within_buckets_and_leaves_gaps() {
        let since = NOW - 900;
        let step = 10;
        let rows = vec![
            row(
                "a",
                since + 1,
                serde_json::json!({"current": 10, "temp_c": 20.0}),
            ),
            row("a", since + 9, serde_json::json!({"current": 30})),
            row(
                "a",
                since + 25,
                serde_json::json!({"current": 7, "rssi_dbm": -60}),
            ),
            row("b", since + 25, serde_json::json!({"current": 999})),
            row("a", since - 1, serde_json::json!({"current": 999})),
            row("a", since + 901, serde_json::json!({"current": 999})),
            row("a", since + 900, serde_json::json!({"current": 50})),
        ];
        let s = bucket_series("a", &rows, since, step);
        assert_eq!(s.points.len(), BUCKETS);
        assert_eq!(
            s.uplinks, 4,
            "other devices and out-of-window rows ignored; t == now kept"
        );
        assert_eq!(
            s.points[BUCKETS - 1].values[0],
            Some(50.0),
            "an uplink at exactly `now` lands in the last bucket"
        );
        assert_eq!(s.points[0].count, 2);
        assert_eq!(s.points[0].values[0], Some(20.0), "mean of 10 and 30");
        assert_eq!(s.points[0].values[1], Some(20.0));
        assert_eq!(s.points[0].values[3], None, "no rssi in bucket 0");
        assert_eq!(s.points[1].count, 0);
        assert!(s.points[1].values.iter().all(Option::is_none));
        assert_eq!(s.points[2].values[0], Some(7.0));
        assert_eq!(s.points[2].values[3], Some(-60.0));
        assert_eq!(s.points[BUCKETS - 1].t, since + 890);
    }

    /// A burst's earlier samples land in the buckets they were taken in;
    /// the uplink instant itself comes from the scalar fields.
    #[test]
    fn bucket_series_expands_level_bursts_backwards_in_time() {
        let since = NOW - 900;
        let step = 10;
        // Three samples 10 s apart ending at since+100: taken at +80, +90, +100.
        let lv_a = METRICS[0].quantize(1_000.0);
        let lv_b = METRICS[0].quantize(2_000.0);
        let mut flat = vec![0u8; METRICS.len() * 3];
        flat[0] = lv_a; // sample 0, current
        flat[METRICS.len()] = lv_b; // sample 1, current
        flat[METRICS.len() + 1] = METRICS[1].quantize(25.0); // sample 1, temp
        flat[METRICS.len() * 2] = 77; // sample 2 is superseded by the scalars
        let rows = vec![row(
            "a",
            since + 100,
            serde_json::json!({ "current": 3000, "levels": flat, "levels_ms": 10_000 }),
        )];
        let s = bucket_series("a", &rows, since, step);
        assert_eq!(s.uplinks, 1, "rows, not samples");
        assert_eq!(s.points[8].count, 0, "count stays per row");
        let v = s.points[8].values[0].unwrap();
        assert!(
            (v - 1_000.0).abs() <= f64::from(METRICS[0].step()) / 2.0,
            "{v}"
        );
        assert_eq!(s.points[8].values[1], None, "level 0 = absent");
        let v = s.points[9].values[0].unwrap();
        assert!(
            (v - 2_000.0).abs() <= f64::from(METRICS[0].step()) / 2.0,
            "{v}"
        );
        let v = s.points[9].values[1].unwrap();
        assert!(
            (v - 25.0).abs() <= f64::from(METRICS[1].step()) / 2.0,
            "{v}"
        );
        assert_eq!(s.points[10].count, 1);
        assert_eq!(
            s.points[10].values[0],
            Some(3_000.0),
            "uplink instant = exact scalar, not level 77"
        );
        assert_eq!(s.points[7].values[0], None);

        // A burst whose early samples predate the window is clipped there.
        let rows = vec![row(
            "a",
            since + 5,
            serde_json::json!({ "current": 1, "levels": flat, "levels_ms": 10_000 }),
        )];
        let s = bucket_series("a", &rows, since, step);
        assert_eq!(s.uplinks, 1);
        assert_eq!(s.points[0].values[0], Some(1.0));
    }

    #[test]
    fn bucket_series_ignores_hostile_payload_values() {
        let since = NOW - 900;
        let rows = vec![
            row("a", since, serde_json::json!({"current": "12"})),
            row("a", since, serde_json::json!({"current": null})),
            row("a", since, serde_json::json!({"current": [1, 2]})),
            row("a", since, serde_json::json!({"current": 1e308})),
            row("a", since, serde_json::json!({"current": -1e308})),
            row("a", since, serde_json::json!({"current": 4})),
            UplinkRecord {
                device_id: "a".into(),
                received_at: "garbage".into(),
                payload: serde_json::json!({"current": 1}),
            },
        ];
        let s = bucket_series("a", &rows, since, 10);
        // 1e308 + -1e308 + 4 over three numeric values: finite, no NaN.
        let v = s.points[0].values[0].unwrap();
        assert!(v.is_finite(), "{v}");
        assert_eq!(s.uplinks, 6, "unparseable timestamp row dropped");
    }

    #[test]
    fn point_serializes_metric_keys_flat() {
        let p = Point {
            t: 5,
            count: 1,
            values: [Some(1.5), None, None, None, Some(2.0)],
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["t"], 5);
        assert_eq!(json["current"], 1.5);
        assert_eq!(json["temp_c"], serde_json::Value::Null);
        assert_eq!(json["heap_free"], 2.0);
    }

    #[test]
    fn timeline_tracks_fresh_stale_dark_never_over_time() {
        let step = Window::H6.step(); // 240 s
        let since = NOW - step * BUCKETS as u64;
        let devices = vec![
            device("fresh-throughout", "active"),
            device("goes-quiet", "active"),
            device("old-timer", "active"),
            device("newcomer", "active"),
            device("revoked", "inactive"),
        ];
        let mut rows = Vec::new();
        // fresh-throughout: every 60 s across the whole window
        let mut t = since - DARK_SECS;
        while t <= NOW {
            rows.push(row("fresh-throughout", t, serde_json::json!({})));
            t += 60;
        }
        // goes-quiet: last uplink 3 h before now
        rows.push(row("goes-quiet", NOW - 3 * 3600, serde_json::json!({})));
        // newcomer: first ever uplink 10 min before now
        rows.push(row("newcomer", NOW - 600, serde_json::json!({})));
        // revoked: uplinks constantly, must not be counted
        rows.push(row("revoked", NOW - 1, serde_json::json!({})));
        let mut latest = HashMap::new();
        latest.insert("fresh-throughout".to_string(), NOW);
        latest.insert("goes-quiet".to_string(), NOW - 3 * 3600);
        latest.insert("old-timer".to_string(), since - DARK_SECS - 86_400);
        latest.insert("newcomer".to_string(), NOW - 600);
        latest.insert("revoked".to_string(), NOW - 1);

        let tl = status_timeline(&devices, &latest, &rows, since, step);
        assert_eq!(tl.len(), BUCKETS);
        for p in &tl {
            assert_eq!(p.total(), 4, "only active devices: {p:?}");
        }
        let first = tl[0];
        let last = tl[BUCKETS - 1];
        // Start of window (6 h ago): fresh-throughout fresh, goes-quiet
        // fresh (its uplink is 3 h from now = 3 h in the future... no:
        // its only uplink is at NOW-3h, after the first bucket) → never
        // yet; old-timer dark; newcomer never.
        assert_eq!(first.fresh, 1, "{first:?}");
        assert_eq!(first.dark, 1, "{first:?}");
        assert_eq!(first.never, 2, "{first:?}");
        // End: fresh-throughout + newcomer fresh, goes-quiet dark (3 h >
        // DARK_SECS), old-timer dark.
        assert_eq!(last.fresh, 2, "{last:?}");
        assert_eq!(last.dark, 2, "{last:?}");
        assert_eq!(last.never, 0, "{last:?}");
        // goes-quiet passes through stale in between.
        let stale_seen = tl.iter().any(|p| p.stale == 1);
        assert!(stale_seen, "goes-quiet never showed as stale");
        // Timeline is monotone in time.
        assert!(tl.windows(2).all(|w| w[1].t == w[0].t + step));
    }

    #[test]
    fn timeline_fresh_boundary_is_exact() {
        let step = 10;
        let since = NOW - step * BUCKETS as u64;
        let devices = vec![device("a", "active")];
        // Uplink exactly FRESH_SECS before the end of the last bucket → stale.
        let rows = vec![row("a", NOW - FRESH_SECS, serde_json::json!({}))];
        let latest = HashMap::from([("a".to_string(), NOW - FRESH_SECS)]);
        let tl = status_timeline(&devices, &latest, &rows, since, step);
        assert_eq!(tl[BUCKETS - 1].stale, 1, "{:?}", tl[BUCKETS - 1]);
        // One second later it was still fresh.
        assert_eq!(tl[BUCKETS - 2].fresh, 1, "{:?}", tl[BUCKETS - 2]);
    }

    #[test]
    fn timeline_with_no_devices_is_all_zero_but_full_length() {
        let tl = status_timeline(&[], &HashMap::new(), &[], 0, 1);
        assert_eq!(tl.len(), BUCKETS);
        assert!(tl.iter().all(|p| p.total() == 0));
    }
}
