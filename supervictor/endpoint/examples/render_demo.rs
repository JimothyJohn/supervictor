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
    bucket_series, power_timeline, status_timeline, History, Window, BUCKETS, METRICS,
};
use supervictor_endpoint::models::{DeviceRecord, UplinkRecord};
use supervictor_endpoint::time::format_rfc3339;
use supervictor_endpoint::ui::views::{self, DeviceHistory, FleetRow, PowerState, Staleness};
use supervictor_endpoint::ui::{assets, views::LIVE_SCRIPT};

/// "Now" for the seeded history: 2026-07-11T04:00:00Z, just after the
/// newest canned uplink.
const DEMO_NOW: u64 = 1_783_828_800;
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
  setInterval(function () {
    var row = rows[Math.floor(Math.random() * rows.length)];
    var last = row.querySelector('.last-uplink');
    if (last) last.textContent = new Date().toISOString().replace(/\.\d+Z$/, '+00:00');
    var badge = row.querySelector('.staleness');
    if (badge) { badge.textContent = 'fresh'; badge.className = 'badge staleness fresh'; }
  }, 2500);
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

    let rows = vec![
        FleetRow {
            device: devices[0].clone(),
            last_uplink: Some("2026-07-11T03:58:12+00:00".into()),
            staleness: Staleness::Fresh,
            current: Some(1_124),
            power: PowerState::On,
        },
        FleetRow {
            device: devices[1].clone(),
            last_uplink: Some("2026-07-11T02:31:55+00:00".into()),
            staleness: Staleness::Stale,
            current: Some(12),
            power: PowerState::Off,
        },
        FleetRow {
            device: devices[2].clone(),
            last_uplink: Some("2026-06-28T17:04:40+00:00".into()),
            staleness: Staleness::Dark,
            current: Some(640),
            power: PowerState::Unknown,
        },
        FleetRow {
            device: devices[3].clone(),
            last_uplink: None,
            staleness: Staleness::Unknown,
            current: None,
            power: PowerState::Unknown,
        },
    ];

    let fleet = views::fleet_page(&rows, DEMO_CSRF, DEMO_SUBJECT).into_string();
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
    let timeline = status_timeline(&devices, &latest, &rows, since, step);
    let power = power_timeline(&devices, &rows, since, step, DEFAULT_POWER_THRESHOLD_MA);
    let series: Vec<_> = devices
        .iter()
        .map(|d| bucket_series(&d.device_id, &rows, since, step))
        .collect();
    let fleet_history = History {
        window: DEMO_WINDOW.label(),
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
    };
    let history_html =
        views::history_page(&fleet_history, &summary, DEMO_WINDOW, DEMO_SUBJECT).into_string();
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
            slot,
            power: demo_power(slot),
            current: demo_current(slot),
            threshold_ma: record.effective_threshold_ma(DEFAULT_POWER_THRESHOLD_MA),
            fleet_threshold_ma: DEFAULT_POWER_THRESHOLD_MA,
        };
        let page = views::device_page(record, &device_history, &uplinks, DEMO_CSRF, DEMO_SUBJECT)
            .into_string();
        fs::write(
            outdir.join(format!("{}.html", record.device_id)),
            staticize(page, &ids),
        )
        .expect("write device page");
    }

    fs::write(outdir.join("style.css"), assets::STYLESHEET).expect("write stylesheet");
    println!(
        "demo rendered: {} pages -> {}",
        devices.len() + 2,
        outdir.display()
    );
}
