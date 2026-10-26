//! Render a static, seeded copy of the fleet dashboard for the GitHub Pages
//! demo. The templates are pure functions, so this is just: canned data →
//! Markup → HTML files, with the live-SSE script swapped for a client-side
//! simulator and absolute `/ui/...` paths rewritten to relative ones.
//!
//! Usage: cargo run -p supervictor-endpoint --example render_demo -- <outdir>

use std::fs;
use std::path::Path;

use supervictor_endpoint::fleet::{FleetSummary, DEFAULT_POWER_THRESHOLD_MA};
use supervictor_endpoint::history::{
    bucket_series, power_timeline, status_timeline, Grid, History, Window, BUCKETS, METRICS,
};
use supervictor_endpoint::live::{self, LiveDevice, RegisterInfo};
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::style::{self, Resolved};
use supervictor_endpoint::time::{format_rfc3339, parse_rfc3339_unix_ms};
use supervictor_endpoint::ui::views::{
    self, DeviceHistory, FleetRow, PowerState, Selection, Staleness,
};
use supervictor_endpoint::ui::{assets, views::LIVE_SCRIPT};

/// "Now" for the seeded history: 2026-07-11T04:00:00Z, just after the
/// newest canned uplink.
const DEMO_NOW: u64 = 1_783_742_400;
const DEMO_WINDOW: Window = Window::H1;

const DEMO_SUBJECT: &str = "CN=demo-viewer,OU=admin,O=supervictor";
const DEMO_CSRF: &str = "static-demo";

/// Client-side simulator: fakes a live feed and disables mutating forms.
const DEMO_SCRIPT: &str = r#"
(function () {
  var dot = document.getElementById('live-dot');
  if (dot) { dot.textContent = 'demo'; dot.className = 'connected'; }
  document.querySelectorAll('form').forEach(function (form) {
    form.addEventListener('submit', function (e) {
      e.preventDefault();
      var button = form.querySelector('button');
      if (!button) return;
      var original = button.textContent;
      button.textContent = 'demo only';
      setTimeout(function () { button.textContent = original; }, 1200);
    });
  });
  var rows = Array.prototype.slice.call(document.querySelectorAll('tr[data-device]'));
  if (!rows.length) return;
  // Live page: keep each device's walk going with one synthetic sample
  // every STEP ms, so the strips advance smoothly instead of leaping a
  // burst at a time. (The firmware ships 50 of these per 5 s uplink; the
  // page draws either the same way.)
  var live = document.getElementById('live');
  var seed = null;
  if (live) { try { seed = JSON.parse(live.dataset.live); } catch (_) { seed = null; } }
  // The page aligns its clock to the seed's now_ms, so synthetic events
  // must be stamped on that virtual clock, not the real one.
  var walks = {}, vels = {}, homes = {}, loaded = Date.now();
  if (seed) {
    seed.devices.forEach(function (d) {
      var first = d.samples.length ? d.samples[0].l : null;
      var last = d.samples.length ? d.samples[d.samples.length - 1].l : null;
      if (last) { walks[d.device_id] = last.slice(); vels[d.device_id] = last.map(function () { return 0; }); homes[d.device_id] = first.slice(); }
    });
  }
  // Every ROW_EVERY samples a random fleet row is marked fresh, the old
  // per-uplink cadence.
  var STEP = 100, ROW_EVERY = 25, ticks = 0;
  function sample(id) {
    var w = walks[id], v = vels[id], home = homes[id], R = w.length, out = [];
    for (var r = 0; r < R; r++) {
      // Smooth wander: velocity with momentum driven by noise, pulled back
      // toward the seed's starting level; an inrush spike on current about
      // every 20 s. Same tuning as the seeded history (demo_live_samples).
      var sigma = r === 0 ? 0.6 : 0.06;
      v[r] = v[r] * 0.9 + (Math.random() - 0.5) * sigma;
      var lv = w[r] + v[r] - (w[r] - home[r]) / 120;
      if (r === 0 && Math.random() < 0.005) lv += 40;
      w[r] = Math.max(1, Math.min(255, lv));
      out.push(Math.round(w[r]));
    }
    return out;
  }
  setInterval(function () {
    ticks += 1;
    if (ticks % ROW_EVERY === 0) {
      var row = rows[Math.floor(Math.random() * rows.length)];
      var last = row.querySelector('.last-uplink');
      if (last) last.textContent = new Date().toISOString().replace(/\.\d+Z$/, '+00:00');
      var badge = row.querySelector('.staleness');
      if (badge) { badge.textContent = 'fresh'; badge.className = 'badge staleness fresh'; }
    }
    Object.keys(walks).forEach(function (wid) {
      document.dispatchEvent(new CustomEvent('sv:uplink', { detail: {
        device_id: wid, t_ms: seed.now_ms + (Date.now() - loaded), levels: sample(wid), levels_ms: STEP
      } }));
    });
  }, STEP);
})();
"#;

fn device(id: &str, status: &str, subject: Option<&str>, created: &str) -> DeviceRecord {
    DeviceRecord {
        device_id: id.into(),
        owner_id: "advin".into(),
        subject_dn: subject.map(Into::into),
        status: status.into(),
        created_at: created.into(),
        power_threshold_ma: None,
        style: Default::default(),
        desired_fw: None,
    }
}

fn uplink(id: &str, at: &str, current: i32) -> UplinkRecord {
    UplinkRecord {
        device_id: id.into(),
        received_at: at.into(),
        payload: serde_json::json!({ "current": current }),
    }
}

/// Rewrite app-absolute paths and the SSE script for static hosting; the
/// window filters all point at the one rendered page, and the periodic
/// reload is dropped (nothing changes on a static site).
fn staticize(html: String, devices: &[&str]) -> String {
    let mut out = html
        .replace("href=\"/ui/assets/style.css\"", "href=\"style.css\"")
        .replace("href=\"/ui\"", "href=\"index.html\"")
        .replace("action=\"/ui/devices\"", "action=\"#\"")
        .replace(" data-refresh=\"60\"", "")
        .replace(LIVE_SCRIPT, DEMO_SCRIPT);
    for w in Window::ALL {
        out = out.replace(
            &format!("href=\"/ui/history?window={}\"", w.label()),
            "href=\"history.html\"",
        );
    }
    out = out.replace("href=\"/ui/history\"", "href=\"history.html\"");
    // Device chips: the query is ignored on a static site, so every
    // selection link is the one rendered page.
    out = out.replace("href=\"/ui/history?", "href=\"history.html?");
    out = out
        .replace("href=\"/ui/live\"", "href=\"live.html\"")
        .replace(" data-poll=\"/ui/live.json\"", "");
    for id in devices {
        for w in Window::ALL {
            out = out.replace(
                &format!("href=\"/ui/devices/{id}?window={}\"", w.label()),
                &format!("href=\"{id}.html\""),
            );
        }
        out = out
            .replace(
                &format!("href=\"/ui/devices/{id}\""),
                &format!("href=\"{id}.html\""),
            )
            .replace(
                &format!("action=\"/ui/devices/{id}/status\""),
                "action=\"#\"",
            )
            .replace(
                &format!("action=\"/ui/devices/{id}/threshold\""),
                "action=\"#\"",
            )
            .replace(
                &format!("action=\"/ui/devices/{id}/style\""),
                "action=\"#\"",
            );
    }
    out
}

/// Deterministic telemetry for the demo: a tiny LCG per device, one uplink
/// every 30 s across the hour, with the same shape the firmware simulator
/// produces (drifting current, slow temperature, jittery RSSI, leaking heap).
fn demo_rows(id: &str, seed: u64, step_s: u64) -> Vec<UplinkRecord> {
    let mut state = seed;
    let mut next = move |n: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    let base = 400 + (seed % 7) * 180;
    let mut rows = Vec::new();
    let mut t = DEMO_NOW - 3600;
    let mut temp = 22_000 + next(6_000) as i64;
    let mut heap = 96_000u64;
    while t <= DEMO_NOW {
        let current = base as i64 + next(60) as i64 - 30 + if next(40) == 0 { 300 } else { 0 };
        temp += next(300) as i64 - 150;
        heap = heap.saturating_sub(next(40));
        rows.push(uplink_full(
            id,
            &format_rfc3339(t, 0),
            current as i32,
            temp as f64 / 1000.0,
            40.0 + next(80) as f64 / 10.0,
            -60 - next(8) as i64,
            heap,
        ));
        t += step_s;
    }
    rows
}

/// Two minutes of 8-bit samples for the live page: one burst of fifty per
/// 5 s uplink (100 ms apart, the firmware default), each register a
/// bounded walk in level space, run through
/// the real burst expansion so the demo exercises the same code path.
fn demo_live_samples(seed: u64, now_ms: u64) -> Vec<live::Sample> {
    let mut state = seed;
    let mut next = move |n: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    let home: Vec<f64> = vec![60.0 + (seed % 7) as f64 * 15.0, 140.0, 110.0, 120.0, 200.0];
    let mut level = home.clone();
    let mut vel = vec![0.0f64; home.len()];
    let mut out = Vec::new();
    let mut t = now_ms - live::WINDOW_S * 1_000;
    while t <= now_ms {
        let mut flat = Vec::with_capacity(50 * METRICS.len());
        for _ in 0..50 {
            for r in 0..home.len() {
                // Smooth wander per 100 ms sample: a velocity with momentum
                // driven by noise, pulled back toward home on a ~12 s time
                // constant. The current register also takes an instant
                // inrush spike about every 20 s that the pull-back then
                // decays — the SEN0211 module's own shape. Mirrored in
                // DEMO_SCRIPT's sample().
                let sigma = if r == 0 { 0.6 } else { 0.06 };
                let noise = (next(1_000) as f64 / 1_000.0 - 0.5) * sigma;
                vel[r] = vel[r] * 0.9 + noise;
                level[r] += vel[r] - (level[r] - home[r]) / 120.0;
                if r == 0 && next(200) == 0 {
                    level[r] += 40.0;
                }
                level[r] = level[r].clamp(1.0, 255.0);
                flat.push(level[r].round() as u8);
            }
        }
        let record = UplinkRecord {
            device_id: "demo".into(),
            received_at: format_rfc3339(t / 1_000, (t % 1_000) as u32 * 1_000_000),
            payload: serde_json::json!({ "current": 1, "levels": flat, "levels_ms": 100 }),
        };
        debug_assert_eq!(parse_rfc3339_unix_ms(&record.received_at), Some(t));
        out.extend(live::samples_of(&record));
        t += 5_000;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn uplink_full(
    id: &str,
    at: &str,
    current: i32,
    temp_c: f64,
    humidity_pct: f64,
    rssi_dbm: i64,
    heap_free: u64,
) -> UplinkRecord {
    UplinkRecord {
        device_id: id.into(),
        received_at: at.into(),
        payload: serde_json::json!({
            "current": current, "fw": "0.1.0", "temp_c": temp_c,
            "humidity_pct": humidity_pct, "rssi_dbm": rssi_dbm, "heap_free": heap_free,
        }),
    }
}

fn demo_power(slot: usize) -> PowerState {
    match slot {
        0 => PowerState::On,
        1 => PowerState::Off,
        _ => PowerState::Unknown,
    }
}

fn demo_current(slot: usize) -> Option<i64> {
    match slot {
        0 => Some(1_124),
        1 => Some(12),
        2 => Some(640),
        _ => None,
    }
}

fn main() {
    let outdir = std::env::args().nth(1).unwrap_or_else(|| "demo-out".into());
    let outdir = Path::new(&outdir);
    fs::create_dir_all(outdir).expect("create output dir");

    let devices = [
        device(
            "factory-01",
            "active",
            Some("CN=factory-01,OU=devices,O=supervictor"),
            "2026-05-02T14:11:09+00:00",
        ),
        device(
            "factory-02",
            "active",
            Some("CN=factory-02,OU=devices,O=supervictor"),
            "2026-05-02T14:26:44+00:00",
        ),
        device(
            "warehouse-07",
            "inactive",
            Some("CN=warehouse-07,OU=devices,O=supervictor"),
            "2026-03-19T09:02:31+00:00",
        ),
        device("lab-bench-3", "active", None, "2026-07-09T21:47:00+00:00"),
    ];
    let ids: Vec<&str> = devices.iter().map(|d| d.device_id.as_str()).collect();
    let styles = style::resolve(&devices);
    let style_of = |id: &str| -> Resolved { styles[id].clone() };

    let rows = vec![
        FleetRow {
            device: devices[0].clone(),
            style: style_of(&devices[0].device_id),
            last_uplink: Some("2026-07-11T03:58:12+00:00".into()),
            staleness: Staleness::Fresh,
            current: Some(1_124),
            power: PowerState::On,
            threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        },
        FleetRow {
            device: devices[1].clone(),
            style: style_of(&devices[1].device_id),
            last_uplink: Some("2026-07-11T02:31:55+00:00".into()),
            staleness: Staleness::Stale,
            current: Some(12),
            power: PowerState::Off,
            threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        },
        FleetRow {
            device: devices[2].clone(),
            style: style_of(&devices[2].device_id),
            last_uplink: Some("2026-06-28T17:04:40+00:00".into()),
            staleness: Staleness::Dark,
            current: Some(640),
            power: PowerState::Unknown,
            threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        },
        FleetRow {
            device: devices[3].clone(),
            style: style_of(&devices[3].device_id),
            last_uplink: None,
            staleness: Staleness::Unknown,
            current: None,
            power: PowerState::Unknown,
            threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        },
    ];

    let fleet = views::fleet_page(&rows, DEMO_CSRF, DEMO_SUBJECT, DEMO_NOW).into_string();
    fs::write(outdir.join("index.html"), staticize(fleet, &ids)).expect("write fleet page");

    // History page: an hour of seeded telemetry for the two live devices.
    let step = DEMO_WINDOW.step();
    let since = DEMO_NOW - step * BUCKETS as u64;
    let mut rows = demo_rows("factory-01", 11, 30);
    rows.extend(demo_rows("factory-02", 23, 30));
    let latest = rows
        .iter()
        .fold(std::collections::HashMap::new(), |mut m, r| {
            m.insert(r.device_id.clone(), DEMO_NOW);
            m
        });
    let timeline = status_timeline(&devices, &latest, &rows, Grid::standard(since, step));
    let power = power_timeline(
        &devices,
        &rows,
        Grid::standard(since, step),
        DEFAULT_POWER_THRESHOLD_MA,
    );
    let series: Vec<_> = devices
        .iter()
        .map(|d| bucket_series(&d.device_id, &rows, Grid::standard(since, step)))
        .collect();
    let fleet_history = History {
        window: DEMO_WINDOW.label(),
        live: DEMO_WINDOW.is_live(),
        buckets: BUCKETS,
        threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        since: format_rfc3339(since, 0),
        until: format_rfc3339(DEMO_NOW, 0),
        step_s: step,
        timeline,
        power,
        devices: series.clone(),
        truncated: false,
    };
    let summary = FleetSummary {
        total: 4,
        active: 3,
        inactive: 1,
        fresh: 2,
        stale: 0,
        dark: 0,
        never: 1,
        on: 2,
        off: 0,
        threshold_overrides: 0,
    };
    // The two reporting devices start selected so the demo has traces.
    let selected: Vec<String> = vec!["factory-01".into(), "factory-02".into()];
    let history_html = views::history_page(
        &fleet_history,
        &summary,
        DEMO_WINDOW,
        DEMO_SUBJECT,
        &Selection {
            styles: &styles,
            selected: &selected,
        },
    )
    .into_string();
    fs::write(outdir.join("history.html"), staticize(history_html, &ids))
        .expect("write history page");
    assert_eq!(METRICS.len(), series[0].points[0].values.len());

    // Detail pages: a lively one for factory-01, sparser ones for the rest.
    let history = [
        ("2026-07-11T03:58:12+00:00", 41),
        ("2026-07-11T03:53:11+00:00", 44),
        ("2026-07-11T03:48:14+00:00", 39),
        ("2026-07-11T03:43:09+00:00", 47),
        ("2026-07-11T03:38:13+00:00", 52),
        ("2026-07-11T03:33:10+00:00", 45),
        ("2026-07-11T03:28:12+00:00", 38),
        ("2026-07-11T03:23:08+00:00", 42),
    ];
    for (slot, record) in devices.iter().enumerate() {
        let uplinks: Vec<UplinkRecord> = if record.device_id == "factory-01" {
            history
                .iter()
                .map(|(at, current)| uplink("factory-01", at, *current))
                .collect()
        } else if record.device_id == "factory-02" {
            vec![uplink("factory-02", "2026-07-11T02:31:55+00:00", 12)]
        } else {
            Vec::new()
        };
        let device_history = DeviceHistory {
            series: &series[slot],
            window: DEMO_WINDOW,
            truncated: false,
            style: style_of(&record.device_id),
            power: demo_power(slot),
            current: demo_current(slot),
            threshold_ma: record.effective_threshold_ma(DEFAULT_POWER_THRESHOLD_MA),
            fleet_threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        };
        let page = views::device_page(
            record,
            &device_history,
            &uplinks,
            DEMO_CSRF,
            DEMO_SUBJECT,
            DEMO_NOW,
        )
        .into_string();
        fs::write(
            outdir.join(format!("{}.html", record.device_id)),
            staticize(page, &ids),
        )
        .expect("write device page");
    }

    // Live page: the last two minutes of the two live devices as 8-bit
    // bursts (50 samples per 5 s uplink), stamped so the page's clock
    // alignment puts them at "now" on first paint.
    let now_ms = DEMO_NOW * 1_000;
    let live_devices: Vec<LiveDevice> = {
        let mut ids: Vec<&str> = ids.clone();
        ids.sort_unstable();
        ids.iter()
            .map(|id| LiveDevice {
                device_id: id.to_string(),
                style: style_of(id),
                samples: match *id {
                    "factory-01" => demo_live_samples(11, now_ms),
                    "factory-02" => demo_live_samples(23, now_ms),
                    _ => Vec::new(),
                },
            })
            .collect()
    };
    let snapshot = live::Snapshot {
        now_ms,
        window_s: live::WINDOW_S,
        registers: METRICS.iter().map(RegisterInfo::from).collect(),
        devices: live_devices,
    };
    let live_html = views::live_page(&snapshot, DEMO_SUBJECT).into_string();
    fs::write(outdir.join("live.html"), staticize(live_html, &ids)).expect("write live page");

    fs::write(outdir.join("style.css"), assets::STYLESHEET).expect("write stylesheet");
    println!(
        "demo rendered: {} pages -> {}",
        devices.len() + 3,
        outdir.display()
    );
}
