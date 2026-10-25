//! maud templates — pure functions from data to `Markup`. All interpolated
//! values are auto-escaped by maud; `PreEscaped` appears exactly once, for a
//! static script constant that contains no user data.

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::fleet::FleetSummary;
pub use crate::fleet::{PowerState, Staleness};
use crate::history::{DeviceSeries, History, Window, METRICS};
use crate::models::{DeviceRecord, UplinkRecord};
use crate::ui::charts::{self, Reference, Series};
use supervictor_common::status;

/// One row of the fleet table.
pub struct FleetRow {
    /// The device record.
    pub device: DeviceRecord,
    /// Most recent uplink timestamp, if any.
    pub last_uplink: Option<String>,
    /// Freshness classification of `last_uplink`.
    pub staleness: Staleness,
    /// Last reported current in mA, if any.
    pub current: Option<i64>,
    /// Power state from `current` against the fleet threshold.
    pub power: PowerState,
}

/// Static SSE hookup — live row updates, with a reload fallback where SSE
/// doesn't survive the proxy (Lambda/API GW buffers responses).
/// Public so the static-demo renderer (`examples/render_demo.rs`) can swap it
/// for a simulated feed on the GitHub Pages demo.
pub const LIVE_SCRIPT: &str = r#"
(function () {
  var dot = document.getElementById('live-dot');
  var errors = 0;
  var es = new EventSource('/ui/events');
  es.onopen = function () { errors = 0; if (dot) { dot.textContent = 'live'; dot.className = 'connected'; } };
  es.addEventListener('uplink', function (e) {
    var ev; try { ev = JSON.parse(e.data); } catch (_) { return; }
    var row = document.querySelector('tr[data-device="' + CSS.escape(ev.device_id) + '"]');
    if (!row) return;
    var last = row.querySelector('.last-uplink');
    if (last) last.textContent = ev.received_at;
    var badge = row.querySelector('.staleness');
    if (badge) { badge.textContent = 'fresh'; badge.className = 'badge staleness fresh'; }
  });
  es.onerror = function () {
    errors += 1;
    if (dot) { dot.textContent = 'offline'; dot.className = ''; }
    if (errors >= 3) { es.close(); setTimeout(function () { location.reload(); }, 30000); }
  };
})();
"#;

/// Hover layer for every `figure.chart` (crosshair + one tooltip listing
/// every series at that bucket; arrow keys do the same from the keyboard)
/// and a visible-tab-only reload on pages that set `data-refresh`. All
/// text goes in via `textContent`; the chart data comes from the escaped
/// `data-chart` attribute the server rendered.
pub const CHART_SCRIPT: &str = r#"
(function () {
  function fmt(v) {
    if (v === null || v === undefined) return '–';
    var a = Math.abs(v);
    if (a >= 1e6) return (v / 1e6).toFixed(1) + 'M';
    if (a >= 1e4) return Math.round(v / 1e3) + 'K';
    if (a >= 100 || v === Math.round(v)) return Math.round(v).toString();
    if (a >= 10) return v.toFixed(1);
    return v.toFixed(2);
  }
  function when(t) { return new Date(t * 1000).toISOString().slice(0, 19).replace('T', ' ') + ' UTC'; }
  document.querySelectorAll('figure.chart[data-chart]').forEach(function (fig) {
    var data; try { data = JSON.parse(fig.dataset.chart); } catch (_) { return; }
    var svg = fig.querySelector('svg'), tip = fig.querySelector('.tooltip'), hair = fig.querySelector('.crosshair');
    var n = data.t.length;
    if (!svg || !tip || !hair || !n) return;
    var stack = data.kind === 'stack', cur = n - 1;
    function xOf(i) { return stack ? data.ml + (i + 0.5) * data.pw / n : data.ml + i * data.pw / Math.max(n - 1, 1); }
    function idxAt(clientX) {
      var r = svg.getBoundingClientRect();
      var xv = (clientX - r.left) / r.width * data.w;
      var i = stack ? Math.floor((xv - data.ml) / (data.pw / n)) : Math.round((xv - data.ml) / (data.pw / Math.max(n - 1, 1)));
      return Math.max(0, Math.min(n - 1, i));
    }
    function render(i, clientX) {
      cur = i;
      hair.setAttribute('x1', xOf(i)); hair.setAttribute('x2', xOf(i)); hair.classList.add('on');
      while (tip.firstChild) tip.removeChild(tip.firstChild);
      var head = document.createElement('div'); head.className = 'tip-time'; head.textContent = when(data.t[i]); tip.appendChild(head);
      data.series.forEach(function (s) {
        var row = document.createElement('div'); row.className = 'tip-row';
        var key = document.createElement('span'); key.className = 'key ' + (stack ? '' : 'line-key ') + s.slot;
        var val = document.createElement('strong'); var v = s.values[i];
        val.textContent = fmt(v) + (v === null || v === undefined ? '' : ' ' + data.unit);
        var lab = document.createElement('span'); lab.className = 'tip-label'; lab.textContent = s.label;
        row.appendChild(key); row.appendChild(val); row.appendChild(lab); tip.appendChild(row);
      });
      tip.hidden = false;
      var r = svg.getBoundingClientRect(), fr = fig.getBoundingClientRect();
      var px = (clientX === undefined ? r.left + xOf(i) / data.w * r.width : clientX) - fr.left;
      var left = px + 12;
      if (left + tip.offsetWidth > fr.width) left = px - tip.offsetWidth - 12;
      tip.style.left = Math.max(0, left) + 'px';
      tip.style.top = (r.top - fr.top + 8) + 'px';
    }
    function hide() { tip.hidden = true; hair.classList.remove('on'); }
    svg.addEventListener('pointermove', function (e) { render(idxAt(e.clientX), e.clientX); });
    svg.addEventListener('pointerleave', hide);
    svg.setAttribute('tabindex', '0');
    svg.addEventListener('focus', function () { render(cur); });
    svg.addEventListener('blur', hide);
    svg.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowLeft') { render(Math.max(0, cur - 1)); e.preventDefault(); }
      else if (e.key === 'ArrowRight') { render(Math.min(n - 1, cur + 1)); e.preventDefault(); }
      else if (e.key === 'Escape') { hide(); }
    });
  });
  var refresh = parseInt(document.body.getAttribute('data-refresh') || '0', 10);
  if (refresh > 0) {
    setInterval(function () { if (document.visibilityState === 'visible') location.reload(); }, refresh * 1000);
  }
})();
"#;

/// Theme bootstrap + toggle. Runs in `<head>` so a stored choice applies
/// before first paint (no flash); the click handler attaches once the
/// header exists. The choice is a per-viewer convenience in localStorage,
/// wrapped in try/catch — private windows and blocked storage just fall
/// back to the OS preference.
pub const THEME_SCRIPT: &str = r#"
(function () {
  var KEY = 'sv-theme', root = document.documentElement;
  function stored() { try { return localStorage.getItem(KEY); } catch (_) { return null; } }
  function effective() {
    var t = root.getAttribute('data-theme');
    if (t) return t;
    return (window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches) ? 'light' : 'dark';
  }
  var saved = stored();
  if (saved === 'light' || saved === 'dark') root.setAttribute('data-theme', saved);
  document.addEventListener('DOMContentLoaded', function () {
    var button = document.getElementById('theme-toggle');
    if (!button) return;
    button.addEventListener('click', function () {
      var next = effective() === 'light' ? 'dark' : 'light';
      root.setAttribute('data-theme', next);
      try { localStorage.setItem(KEY, next); } catch (_) {}
      button.setAttribute('aria-label', next === 'light' ? 'switch to dark mode' : 'switch to light mode');
    });
  });
})();
"#;

/// Seconds between reloads of the history pages (visible tab only).
const HISTORY_REFRESH_S: u32 = 60;

fn layout(title: &str, subject: &str, refresh: Option<u32>, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="dark light";
                meta name="theme-color" content="#0e0e10" media="(prefers-color-scheme: dark)";
                meta name="theme-color" content="#f4f4f5" media="(prefers-color-scheme: light)";
                title { (title) " — supervictor fleet" }
                link rel="stylesheet" href="/ui/assets/style.css";
                script { (PreEscaped(THEME_SCRIPT)) }
            }
            body data-refresh=[refresh] {
                header {
                    h1 { a href="/ui" { "supervictor fleet" } }
                    nav {
                        a href="/ui" { "fleet" }
                        a href="/ui/history" { "history" }
                    }
                    span #live-dot { "static" }
                    span .subject { (subject) }
                    button #theme-toggle type="button" aria-label="switch colour theme" title="light / dark" {
                        svg .icon-sun viewBox="0 0 24 24" aria-hidden="true" {
                            circle cx="12" cy="12" r="4" {}
                            path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" {}
                        }
                        svg .icon-moon viewBox="0 0 24 24" aria-hidden="true" {
                            path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" {}
                        }
                    }
                }
                main { (body) }
                script { (PreEscaped(LIVE_SCRIPT)) }
                script { (PreEscaped(CHART_SCRIPT)) }
            }
        }
    }
}

/// Window preset row. `base` is the page path; the current preset is marked.
fn window_filters(base: &str, current: Window) -> Markup {
    html! {
        nav .filters aria-label="time window" {
            span .filters-label { "window" }
            @for w in Window::ALL {
                a .active[w == current] href={ (base) "?window=" (w.label()) } { (w.label()) }
            }
        }
    }
}

/// `GET /ui/history` — status timeline, headline counts, one chart per metric.
pub fn history_page(
    history: &History,
    summary: &FleetSummary,
    window: Window,
    subject: &str,
) -> Markup {
    let times: Vec<u64> = history.timeline.iter().map(|p| p.t).collect();
    let uplinks: usize = history.devices.iter().map(|d| d.uplinks).sum();
    // One column per device per metric, built once.
    let columns: Vec<Vec<Vec<Option<f64>>>> = history
        .devices
        .iter()
        .map(|d| {
            (0..METRICS.len())
                .map(|m| d.points.iter().map(|p| p.values[m]).collect())
                .collect()
        })
        .collect();
    layout(
        "history",
        subject,
        Some(HISTORY_REFRESH_S),
        html! {
            (window_filters("/ui/history", window))
            @if history.truncated {
                p .banner { "row cap reached: the most recent part of this window is not shown — pick a shorter window" }
            }
            section .tiles {
                (charts::stat_tile("active devices", &summary.active.to_string(), &format!("{} registered, {} inactive", summary.total, summary.inactive), ""))
                (charts::stat_tile("powered on", &summary.on.to_string(), &format!("current ≥ {} mA", history.threshold_ma), "on"))
                (charts::stat_tile("powered off", &summary.off.to_string(), &format!("current < {} mA", history.threshold_ma), "off"))
                (charts::stat_tile("fresh", &summary.fresh.to_string(), "heard from in the last 15 min", "fresh"))
                (charts::stat_tile("stale", &summary.stale.to_string(), "quiet 15 min – 2 h", "stale"))
                (charts::stat_tile("dark", &summary.dark.to_string(), "silent over 2 h", "dark"))
                (charts::stat_tile("never", &summary.never.to_string(), "no uplink on record", ""))
                (charts::stat_tile("uplinks", &uplinks.to_string(), &format!("in the last {}", window.label()), ""))
            }
            section .charts {
                @let threshold_label = format!("on ≥ {} mA", history.threshold_ma);
                @for (m, metric) in METRICS.iter().enumerate().take(1) {
                    @let series: Vec<Series<'_>> = history.devices.iter().enumerate().map(|(slot, d)| Series {
                        label: &d.device_id,
                        slot,
                        values: &columns[slot][m],
                    }).collect();
                    div .wide { (charts::line_chart(metric.key, metric.label, metric.unit, &times, &series, window,
                        Some(Reference { value: history.threshold_ma as f64, label: &threshold_label }))) }
                }
                div .wide { (charts::power_chart("power", &history.power, history.threshold_ma, window)) }
                div .wide { (charts::status_chart("status", &history.timeline, window)) }
                @for (m, metric) in METRICS.iter().enumerate().skip(1) {
                    @let series: Vec<Series<'_>> = history.devices.iter().enumerate().map(|(slot, d)| Series {
                        label: &d.device_id,
                        slot,
                        values: &columns[slot][m],
                    }).collect();
                    div { (charts::line_chart(metric.key, metric.label, metric.unit, &times, &series, window, None)) }
                }
            }
        },
    )
}

/// What the device page needs beyond the record: its bucketed series,
/// the window it covers, whether the row cap cut it, and its fleet colour.
pub struct DeviceHistory<'a> {
    /// Bucketed telemetry for the window.
    pub series: &'a DeviceSeries,
    /// Window shown.
    pub window: Window,
    /// Row cap hit.
    pub truncated: bool,
    /// Categorical colour slot (fleet order).
    pub slot: usize,
    /// Power state right now.
    pub power: PowerState,
    /// Last reported current in mA, if any.
    pub current: Option<i64>,
    /// Current (mA) at or above which the device counts as on: its own
    /// override, else the fleet default.
    pub threshold_ma: i64,
    /// The fleet default, shown beside the override form.
    pub fleet_threshold_ma: i64,
}

/// `GET /ui` — fleet table plus the register form.
pub fn fleet_page(rows: &[FleetRow], csrf: &str, subject: &str) -> Markup {
    layout(
        "fleet",
        subject,
        None,
        html! {
            h2 { (rows.len()) " device(s) · " a href="/ui/history" { "status over time" } }
            table {
                thead {
                    tr {
                        th { "device" }
                        th { "status" }
                        th { "power" }
                        th { "owner" }
                        th { "last uplink" }
                        th { "freshness" }
                        th { "actions" }
                    }
                }
                tbody {
                    @for row in rows {
                        tr data-device=(row.device.device_id) {
                            td { a href={ "/ui/devices/" (row.device.device_id) } { (row.device.device_id) } }
                            td { span .badge.(row.device.status) { (row.device.status) } }
                            td {
                                span .badge.power.(row.power.label()) { (row.power.label()) }
                                @if let Some(c) = row.current { " " span .current-inline { (c) " mA" } }
                                @if let Some(t) = row.device.power_threshold_ma { " " span .current-inline title="per-device power threshold" { "≥ " (t) } }
                            }
                            td { (row.device.owner_id) }
                            td .last-uplink { (row.last_uplink.as_deref().unwrap_or("never")) }
                            td { span .badge.staleness.(row.staleness.label()) { (row.staleness.label()) } }
                            td { (status_form(&row.device, csrf)) }
                        }
                    }
                }
            }
            fieldset {
                legend { "register device" }
                form method="post" action="/ui/devices" {
                    input type="hidden" name="csrf" value=(csrf);
                    input type="text" name="device_id" placeholder="device id" required;
                    input type="text" name="owner_id" placeholder="owner id" required;
                    input type="text" name="subject_dn" placeholder="subject DN (optional)";
                    button type="submit" { "register" }
                }
            }
        },
    )
}

/// `GET /ui/devices/{id}` — detail: metric charts over the window, then
/// the most recent raw uplinks.
pub fn device_page(
    device: &DeviceRecord,
    history: &DeviceHistory<'_>,
    uplinks: &[UplinkRecord],
    csrf: &str,
    subject: &str,
) -> Markup {
    let times: Vec<u64> = history.series.points.iter().map(|p| p.t).collect();
    let columns: Vec<Vec<Option<f64>>> = (0..METRICS.len())
        .map(|m| history.series.points.iter().map(|p| p.values[m]).collect())
        .collect();
    let last = uplinks.first();
    let fw = last
        .and_then(|u| u.payload.get("fw"))
        .and_then(|v| v.as_str())
        .unwrap_or("—")
        .to_string();
    let base = format!("/ui/devices/{}", device.device_id);
    layout(
        &device.device_id,
        subject,
        Some(HISTORY_REFRESH_S),
        html! {
            h2 { (device.device_id) }
            dl .meta {
                dt { "status" }
                dd { span .badge.(device.status) { (device.status) } " " (status_form(device, csrf)) }
                dt { "power" }
                dd {
                    span .badge.power.(history.power.label()) { (history.power.label()) }
                    @if let Some(c) = history.current { " " span .current-inline { (c) " mA, threshold " (history.threshold_ma) " mA" } }
                }
                dt { "power threshold" }
                dd { (threshold_form(device, history, csrf)) }
                dt { "owner" }
                dd { (device.owner_id) }
                dt { "cert subject" }
                dd { (device.subject_dn.as_deref().unwrap_or("—")) }
                dt { "registered" }
                dd { (device.created_at) }
            }
            (window_filters(&base, history.window))
            @if history.truncated {
                p .banner { "row cap reached: the most recent part of this window is not shown — pick a shorter window" }
            }
            section .tiles {
                (charts::stat_tile("uplinks", &history.series.uplinks.to_string(), &format!("in the last {}", history.window.label()), ""))
                (charts::stat_tile("last uplink", last.map(|u| u.received_at.as_str()).unwrap_or("never"), "", ""))
                (charts::stat_tile("firmware", &fw, "as last reported", ""))
            }
            section .charts {
                @let threshold_label = format!("on ≥ {} mA", history.threshold_ma);
                @for (m, metric) in METRICS.iter().enumerate() {
                    @let series = [Series { label: &device.device_id, slot: history.slot, values: &columns[m] }];
                    @let reference = (m == 0).then(|| Reference { value: history.threshold_ma as f64, label: &threshold_label });
                    div .wide[m == 0] { (charts::line_chart(metric.key, metric.label, metric.unit, &times, &series, history.window, reference)) }
                }
            }
            h2 { "recent uplinks" }
            @if uplinks.is_empty() {
                p { "no uplinks recorded" }
            } @else {
                table {
                    thead { tr { th { "received" } th { "payload" } } }
                    tbody {
                        @for uplink in uplinks {
                            tr {
                                td { (uplink.received_at) }
                                td { code { (uplink.payload.to_string()) } }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// Revoke/reactivate toggle for a device (CSRF-protected form post).
/// Set or clear the device's power threshold override. The number box is
/// prefilled with the override (empty when the fleet default applies);
/// submitting it empty clears the override.
fn threshold_form(device: &DeviceRecord, history: &DeviceHistory<'_>, csrf: &str) -> Markup {
    let own = device.power_threshold_ma.map(|t| t.to_string());
    html! {
        form .inline method="post" action={ "/ui/devices/" (device.device_id) "/threshold" } {
            input type="hidden" name="csrf" value=(csrf);
            label {
                "on ≥ "
                input type="number" name="threshold_ma" step="1" size="8"
                    value=[own.as_deref()]
                    placeholder=(history.fleet_threshold_ma);
                " mA"
            }
            button type="submit" { "set" }
            @if own.is_some() {
                " " span .muted { "(fleet default " (history.fleet_threshold_ma) " mA; submit empty to clear)" }
            } @else {
                " " span .muted { "(fleet default)" }
            }
        }
    }
}

fn status_form(device: &DeviceRecord, csrf: &str) -> Markup {
    let (next, verb) = if device.status == status::ACTIVE {
        (status::INACTIVE, "revoke")
    } else {
        (status::ACTIVE, "reactivate")
    };
    html! {
        form .inline method="post" action={ "/ui/devices/" (device.device_id) "/status" } {
            input type="hidden" name="csrf" value=(csrf);
            input type="hidden" name="status" value=(next);
            button type="submit" { (verb) }
        }
    }
}
