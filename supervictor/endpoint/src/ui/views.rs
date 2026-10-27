//! maud templates — pure functions from data to `Markup`. All interpolated
//! values are auto-escaped by maud; `PreEscaped` is used only for the static
//! script constants below, which contain no user data (page data reaches
//! them through escaped `data-*` attributes).
//!
//! Every page is two layers of one document. The desktop is the command
//! centre: tables, every chart, every form. A phone is a check on the go
//! — is the fleet all right, is that one machine on — so each page also
//! renders a `.glance` block (one verdict, tappable rows, one chart drawn
//! at phone width) and wraps the dense material in `.deep`. The
//! stylesheet shows one layer or the other by viewport; the phone's
//! "show everything" toggle (`data-view="full"` on `<html>`) lifts the
//! `.deep` layer when a point check turns into real work.

use std::collections::HashMap;

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::fleet::FleetSummary;
pub use crate::fleet::{PowerState, Staleness};
use crate::history::{DeviceSeries, History, Window, METRICS};
use crate::live;
use crate::models::{DeviceRecord, UplinkRecord};
use crate::style::{Color, LineStyle, Marker, Resolved, SLOTS};
use crate::time::parse_rfc3339_unix;
use crate::ui::assets;
use crate::ui::charts::{self, Frame, Reference, Series};
use crate::ui::themes;
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
    /// Power state from `current` against `threshold_ma`.
    pub power: PowerState,
    /// Current (mA) this device is judged against (own override or fleet
    /// default); carried on the row so the live script can flip the badge.
    pub threshold_ma: i64,
    /// How the charts draw this device (the key beside its name).
    pub style: Resolved,
}

/// How a page keeps itself current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMode {
    /// No reload; the SSE script patches rows in place (fleet table).
    Rows,
    /// Reload the visible tab every N seconds (long windows).
    Timed(u32),
    /// Re-render `<main>` on every uplink event (short windows).
    Live,
}

impl PageMode {
    /// Live for the short windows, timed reload otherwise.
    pub fn for_window(window: Window) -> PageMode {
        if window.is_live() {
            PageMode::Live
        } else {
            PageMode::Timed(HISTORY_REFRESH_S)
        }
    }
}

/// Static SSE hookup. One `EventSource` per page: it keeps the header dot
/// honest, patches the fleet table's row for the device that just pinged,
/// and re-dispatches every uplink as a `sv:uplink` DOM event for whatever
/// else is listening (the live strips). Where SSE doesn't survive a
/// buffering proxy, a page that declares `data-poll` on
/// `#live` falls back to polling that snapshot URL (`sv:snapshot`); other
/// pages fall back to a periodic reload.
/// Public so the static-demo renderer (`examples/render_demo.rs`) can swap it
/// for a simulated feed on the GitHub Pages demo.
pub const LIVE_SCRIPT: &str = r#"
(function () {
  var dot = document.getElementById('live-dot');
  var live = document.body.getAttribute('data-live') === '1';
  var strips = document.getElementById('live');
  var errors = 0, pending = null, inflight = false, dirty = false, poll = null;
  function setDot(text, cls) { if (dot) { dot.textContent = text; dot.className = cls; } }
  function emit(name, detail) { document.dispatchEvent(new CustomEvent(name, { detail: detail })); }
  function flash(el) { el.classList.remove('tick'); void el.offsetWidth; el.classList.add('tick'); }
  // Device chips (history page): the selection lives in the URL
  // (`device=` keys, so a link reproduces the view) and is remembered per
  // viewer. A click toggles the chip, rewrites the URL in place and
  // re-renders <main>; without script the chip is a plain link.
  var chips = document.querySelector('nav.devices');
  var SEL = 'sv-devices';
  function selected() {
    var out = [];
    new URLSearchParams(location.search).getAll('device').forEach(function (id) { if (id) out.push(id); });
    return out;
  }
  function remember(ids) { try { localStorage.setItem(SEL, JSON.stringify(ids)); } catch (_) {} }
  if (chips) {
    // First visit without a selection in the URL: restore the remembered one.
    if (!new URLSearchParams(location.search).has('device')) {
      var stored = null;
      try { stored = JSON.parse(localStorage.getItem(SEL) || 'null'); } catch (_) { stored = null; }
      if (Array.isArray(stored) && stored.length) {
        var params = new URLSearchParams(location.search);
        stored.forEach(function (id) { if (typeof id === 'string' && id) params.append('device', id); });
        location.replace(location.pathname + '?' + params.toString());
      }
    }
    chips.addEventListener('click', function (e) {
      var chip = e.target.closest('a.chip');
      if (!chip || !chips.contains(chip)) return;
      e.preventDefault();
      history.replaceState(null, '', chip.getAttribute('href'));
      chip.classList.toggle('on');
      chip.setAttribute('aria-pressed', chip.classList.contains('on') ? 'true' : 'false');
      remember(selected());
      schedule();
    });
  }
  // Fleet table row and glance row alike (anything wearing the device id):
  // patch in place — last uplink, freshness, current, and the power badge
  // judged against the row's own threshold.
  function patchRow(ev) {
    document.querySelectorAll('[data-device="' + CSS.escape(ev.device_id) + '"]').forEach(function (row) {
      var last = row.querySelector('.last-uplink');
      if (last) last.textContent = ev.received_at;
      var badge = row.querySelector('.staleness');
      if (badge) { badge.textContent = 'fresh'; badge.className = 'badge staleness fresh'; }
      var when = row.querySelector('.when');
      if (when) { when.textContent = 'just now'; when.className = 'when fresh'; }
      if (typeof ev.current === 'number') {
        var now = row.querySelector('.current-now');
        if (now) now.textContent = ev.current + ' mA';
        var threshold = parseInt(row.getAttribute('data-threshold'), 10);
        var minOn = parseInt(row.getAttribute('data-min-on'), 10);
        var power = row.querySelector('.badge.power');
        if (power && Number.isFinite(threshold)) {
          var state = ev.current >= threshold ? 'on' : 'off';
          // A minimum on-time is judged over history on the server: one
          // reading can switch a device off, never on. Leave 'on' to the
          // next server render.
          if (state === 'off' || !(minOn > 0)) {
            power.textContent = state; power.className = 'badge power ' + state;
          }
        }
      }
      flash(row);
    });
  }
  // Live pages (short windows): re-render <main> from the server on every
  // uplink. Bursts coalesce into one fetch (150 ms), fetches never overlap,
  // and a form the operator is typing in is never swapped out.
  function rerender() {
    if (inflight) { dirty = true; return; }
    inflight = true;
    fetch(location.href, { credentials: 'same-origin', cache: 'no-store', headers: { 'Accept': 'text/html' } })
      .then(function (r) { if (!r.ok) throw new Error(String(r.status)); return r.text(); })
      .then(function (html) {
        var doc = new DOMParser().parseFromString(html, 'text/html');
        var next = doc.querySelector('main'), cur = document.querySelector('main');
        if (!next || !cur) return;
        var active = document.activeElement;
        if (active && active.closest && active.closest('form') && cur.contains(active)) return;
        // Carry open table twins across the swap; no wash over the page —
        // the data moving is the signal.
        var open = [].map.call(cur.querySelectorAll('details'), function (d) { return d.open; });
        cur.innerHTML = next.innerHTML;
        [].forEach.call(cur.querySelectorAll('details'), function (d, i) { if (open[i]) d.open = true; });
        if (window.svBindCharts) window.svBindCharts(cur);
      })
      .catch(function () {})
      .finally(function () { inflight = false; if (dirty) { dirty = false; schedule(); } });
  }
  function schedule() { clearTimeout(pending); pending = setTimeout(rerender, 150); }
  var es = new EventSource('/ui/events');
  es.onopen = function () { errors = 0; setDot('live', live ? 'connected live' : 'connected'); };
  es.addEventListener('uplink', function (e) {
    var ev; try { ev = JSON.parse(e.data); } catch (_) { return; }
    if (!ev || typeof ev.device_id !== 'string') return;
    patchRow(ev);
    emit('sv:uplink', ev);
    if (live && document.visibilityState === 'visible') schedule();
  });
  es.onerror = function () {
    errors += 1;
    setDot('offline', '');
    if (errors < 3) return;
    es.close();
    var url = strips && strips.getAttribute('data-poll');
    if (!url) { setTimeout(function () { location.reload(); }, live ? 5000 : 30000); return; }
    if (poll) return;
    setDot('polling', 'polling');
    poll = setInterval(function () {
      if (document.visibilityState !== 'visible') return;
      fetch(url, { credentials: 'same-origin' })
        .then(function (r) { return r.ok ? r.json() : null; })
        .then(function (snap) { if (snap) emit('sv:snapshot', snap); })
        .catch(function () {});
    }, 2000);
  };
  if (live) {
    // The time axis keeps moving even while every machine is quiet, and a
    // tab coming back to the foreground catches up at once.
    setInterval(function () { if (document.visibilityState === 'visible') schedule(); }, 15000);
    document.addEventListener('visibilitychange', function () { if (document.visibilityState === 'visible') schedule(); });
  }
})();
"#;

/// The live page: a register bank of meters (real units; the 8-bit level
/// is only the bar width) and one scrolling strip per register, drawn on
/// canvas every animation frame from the samples seeded in
/// `#live[data-live]` plus whatever `sv:uplink` / `sv:snapshot` deliver.
/// Levels are the y axis (1..=255 maps onto each register's fixed range,
/// 0 breaks the line), so the scale never jumps as data arrives; tick
/// labels are dequantised into real units. Each device draws in its
/// resolved style (`crate::style`): palette colours are `--sN` tokens read
/// at draw time so the theme toggle applies immediately, custom colours
/// are literal; dash pattern and per-sample marker follow the record. All
/// text goes in via `textContent`.
pub const LIVE_PAGE_SCRIPT: &str = r#"
(function () {
  var root = document.getElementById('live');
  if (!root) return;
  var seed; try { seed = JSON.parse(root.dataset.live); } catch (_) { return; }
  var REG = seed.registers, R = REG.length, KEEP_MS = seed.window_s * 1000;
  var skew = Date.now() - seed.now_ms;
  var span = 60, devices = {}, order = [];
  seed.devices.forEach(function (d) {
    devices[d.device_id] = { style: d.style || {}, t: d.samples.map(function (s) { return s.t; }), l: d.samples.map(function (s) { return s.l; }) };
    order.push(d.device_id);
  });
  function css(name) { return getComputedStyle(document.documentElement).getPropertyValue(name).trim(); }
  // A palette colour is `var(--sN)`: resolve it against the current theme.
  function paint(style) {
    var c = style.color || '';
    var m = /^var\((--[a-z0-9-]+)\)$/i.exec(c);
    return m ? css(m[1]) : c;
  }
  function dashes(style) { return typeof style.dash === 'string' ? style.dash.split(/\s+/).map(Number).filter(function (n) { return n > 0; }) : []; }
  function glyph(ctx, kind, px, py, r) {
    ctx.beginPath();
    if (kind === 'circle') ctx.arc(px, py, r, 0, Math.PI * 2);
    else if (kind === 'square') ctx.rect(px - r, py - r, 2 * r, 2 * r);
    else if (kind === 'diamond') { ctx.moveTo(px, py - r); ctx.lineTo(px + r, py); ctx.lineTo(px, py + r); ctx.lineTo(px - r, py); ctx.closePath(); }
    else if (kind === 'triangle') { ctx.moveTo(px, py - r); ctx.lineTo(px + r, py + r); ctx.lineTo(px - r, py + r); ctx.closePath(); }
    else return;
    ctx.fill();
  }
  function deq(r, lv) { if (!lv) return null; return REG[r].lo + (lv - 1) * (REG[r].hi - REG[r].lo) / 254; }
  function fmt(v) {
    if (v === null || v === undefined) return '–';
    var a = Math.abs(v);
    if (a >= 1e6) return (v / 1e6).toFixed(1) + 'M';
    if (a >= 1e4) return Math.round(v / 1e3) + 'K';
    if (a >= 100 || v === Math.round(v)) return Math.round(v).toString();
    if (a >= 10) return v.toFixed(1);
    return v.toFixed(2);
  }
  // Samples stay time-ordered: a burst is back-dated from its receive
  // time, so its first samples can land before the previous burst's last
  // ones (receive jitter, or a feed whose bursts overlap). Appending those
  // would send the stroke backwards and the line would double back over
  // itself. A sample at an already-held instant replaces it.
  function push(id, t, l) {
    var d = devices[id], i = d.t.length;
    while (i > 0 && d.t[i - 1] > t) i--;
    if (i > 0 && d.t[i - 1] === t) { d.l[i - 1] = l; }
    else { d.t.splice(i, 0, t); d.l.splice(i, 0, l); }
    var cut = d.t[d.t.length - 1] - KEEP_MS - 10000;
    while (d.t.length && d.t[0] < cut) { d.t.shift(); d.l.shift(); }
  }
  function updateMeters(id) {
    var d = devices[id], last = d.l[d.l.length - 1];
    if (!last) return;
    root.querySelectorAll('[data-device="' + CSS.escape(id) + '"] .meter').forEach(function (m) {
      var r = parseInt(m.dataset.register, 10), lv = last[r] | 0, v = deq(r, lv);
      m.style.setProperty('--lv', lv);
      m.querySelector('.meter-value').textContent = v === null ? '–' : fmt(v);
      if (v === null) { m.removeAttribute('aria-valuenow'); m.removeAttribute('aria-valuetext'); }
      else { m.setAttribute('aria-valuenow', v); m.setAttribute('aria-valuetext', fmt(v) + ' ' + REG[r].unit); }
    });
  }
  function ingest(ev) {
    if (!ev || !devices[ev.device_id] || !Array.isArray(ev.levels)) return;
    var n = Math.floor(ev.levels.length / R);
    if (!n) return;
    var step = ev.levels_ms | 0, t0 = ev.t_ms || (Date.now() - skew);
    for (var i = 0; i < n; i++) push(ev.device_id, t0 - (n - 1 - i) * step, ev.levels.slice(i * R, (i + 1) * R));
    updateMeters(ev.device_id);
  }
  document.addEventListener('sv:uplink', function (e) { ingest(e.detail); });
  document.addEventListener('sv:snapshot', function (e) {
    var snap = e.detail;
    if (!snap || !Array.isArray(snap.devices)) return;
    if (snap.now_ms) skew = Date.now() - snap.now_ms;
    snap.devices.forEach(function (d) {
      var cur = devices[d.device_id];
      if (!cur || !Array.isArray(d.samples)) return;
      cur.t = d.samples.map(function (s) { return s.t; }); cur.l = d.samples.map(function (s) { return s.l; });
      updateMeters(d.device_id);
    });
  });
  var strips = [].slice.call(root.querySelectorAll('figure.strip'));
  // The head of each line trails real time by one sample interval (that
  // device's last spacing, held to 0.1–1 s) and is interpolated between
  // the two samples around the display instant, so a new sample extends
  // the line smoothly across a frame instead of snapping it forward.
  var LAG_MIN = 100, LAG_MAX = 1000;
  // A run of points becomes one path: straight segments when samples sit
  // closer than a few pixels (a curve would be invisible and costs more),
  // otherwise a monotone cubic through them (Fritsch–Carlson tangents:
  // passes every sample, never overshoots a peak or trough).
  var CURVE_PX = 2.5;
  function trace(ctx, pts, curved) {
    var n = pts.length;
    if (!n) return;
    ctx.moveTo(pts[0][0], pts[0][1]);
    if (!curved || n < 3) { for (var k = 1; k < n; k++) ctx.lineTo(pts[k][0], pts[k][1]); return; }
    var dx = [], s = [], m = new Array(n), i;
    for (i = 0; i < n - 1; i++) {
      dx.push(pts[i + 1][0] - pts[i][0]);
      s.push(dx[i] > 0 ? (pts[i + 1][1] - pts[i][1]) / dx[i] : 0);
    }
    m[0] = s[0]; m[n - 1] = s[n - 2];
    for (i = 1; i < n - 1; i++) m[i] = s[i - 1] * s[i] <= 0 ? 0 : (s[i - 1] + s[i]) / 2;
    for (i = 0; i < n - 1; i++) {
      if (!s[i]) { m[i] = 0; m[i + 1] = 0; continue; }
      var a = m[i] / s[i], b = m[i + 1] / s[i], h = a * a + b * b;
      if (h > 9) { var t = 3 / Math.sqrt(h); m[i] = t * a * s[i]; m[i + 1] = t * b * s[i]; }
    }
    for (i = 0; i < n - 1; i++) {
      var d = dx[i] / 3;
      ctx.bezierCurveTo(pts[i][0] + d, pts[i][1] + m[i] * d, pts[i + 1][0] - d, pts[i + 1][1] - m[i + 1] * d, pts[i + 1][0], pts[i + 1][1]);
    }
  }
  function head(t, l, r, j, tau) {
    if (j <= 0 || j >= t.length) return null;
    var a = l[j - 1][r] | 0, b = l[j][r] | 0;
    if (!a || !b) return null;
    var span = t[j] - t[j - 1];
    return span > 0 ? a + (b - a) * (tau - t[j - 1]) / span : b;
  }
  function draw() {
    var now = Date.now() - skew, t1 = now, t0 = now - span * 1000;
    var grid = css('--border'), muted = css('--muted'), surface = css('--surface'), colors = {};
    strips.forEach(function (fig) {
      var r = parseInt(fig.dataset.register, 10), canvas = fig.querySelector('canvas');
      var rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return;
      var dpr = window.devicePixelRatio || 1, W = Math.round(rect.width), H = Math.round(rect.height);
      if (canvas.width !== W * dpr || canvas.height !== H * dpr) { canvas.width = W * dpr; canvas.height = H * dpr; }
      var ctx = canvas.getContext('2d');
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);
      var ml = 56, mr = 14, mt = 8, mb = 18, pw = W - ml - mr, ph = H - mt - mb;
      function x(t) { return ml + (t - t0) / (t1 - t0) * pw; }
      function y(lv) { return mt + (255 - lv) / 254 * ph; }
      function yv(v) { return y(1 + (v - REG[r].lo) / (REG[r].hi - REG[r].lo) * 254); }
      ctx.font = '10px ui-monospace, "SF Mono", Menlo, monospace';
      ctx.lineWidth = 1; ctx.strokeStyle = grid; ctx.fillStyle = muted;
      for (var g = 0; g <= 4; g++) {
        var v = REG[r].lo + (REG[r].hi - REG[r].lo) * g / 4, yy = Math.round(yv(v)) + 0.5;
        ctx.beginPath(); ctx.moveTo(ml, yy); ctx.lineTo(W - mr, yy); ctx.stroke();
        ctx.textAlign = 'right'; ctx.fillText(fmt(v), ml - 6, yy + 3.5);
      }
      for (var k = 0; k <= 4; k++) {
        var tt = t0 + (t1 - t0) * k / 4, ago = Math.round((t1 - tt) / 1000);
        ctx.textAlign = k === 0 ? 'left' : (k === 4 ? 'right' : 'center');
        ctx.fillText(ago ? '-' + ago + 's' : 'now', x(tt), H - 5);
      }
      ctx.save(); ctx.beginPath(); ctx.rect(ml - 1, 0, pw + 2, H); ctx.clip();
      order.forEach(function (id) {
        var d = devices[id], n = d.t.length;
        if (!n) return;
        var lag = n >= 2 ? Math.min(LAG_MAX, Math.max(LAG_MIN, d.t[n - 1] - d.t[n - 2])) : LAG_MIN;
        var tau = now - lag;
        var color = colors[id] || (colors[id] = paint(d.style));
        ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = 2; ctx.lineJoin = 'round'; ctx.lineCap = 'round';
        ctx.setLineDash(dashes(d.style));
        ctx.beginPath();
        var runs = [], run = null, marks = [], i = 0;
        while (i < n && d.t[i] < t0 - 2000) i++;
        var first = i;
        for (; i < n && d.t[i] <= tau; i++) {
          var lv = d.l[i][r] | 0;
          if (!lv) { run = null; continue; }
          var px = x(d.t[i]), py = y(lv);
          if (!run) { run = []; runs.push(run); }
          run.push([px, py]);
          if (d.style.marker && d.style.marker !== 'none') marks.push([px, py]);
        }
        var hv = head(d.t, d.l, r, i, tau);
        if (hv !== null) { if (!run) { run = []; runs.push(run); } run.push([x(tau), y(hv)]); }
        var curved = i - first > 1 && pw / (i - first) >= CURVE_PX;
        for (var q = 0; q < runs.length; q++) trace(ctx, runs[q], curved);
        ctx.stroke();
        ctx.setLineDash([]);
        for (var k = 0; k < marks.length; k++) glyph(ctx, d.style.marker, marks[k][0], marks[k][1], 2.5);
        var tail = runs.length ? runs[runs.length - 1] : null;
        if (tail) {
          var lp = tail[tail.length - 1];
          ctx.beginPath(); ctx.arc(lp[0], lp[1], 3.5, 0, Math.PI * 2); ctx.fill();
          ctx.strokeStyle = surface; ctx.lineWidth = 2; ctx.stroke();
        }
      });
      ctx.restore();
    });
  }
  root.querySelectorAll('button[data-span]').forEach(function (b) {
    b.addEventListener('click', function () {
      span = parseInt(b.dataset.span, 10) || 60;
      root.querySelectorAll('button[data-span]').forEach(function (o) { o.classList.toggle('active', o === b); });
    });
  });
  function frame() { if (document.visibilityState === 'visible') draw(); requestAnimationFrame(frame); }
  requestAnimationFrame(frame);
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
  // Line-series key for the tooltip: the same line sample + marker as the
  // legend, built from the series' style fields (colour is a CSS value).
  var NS = 'http://www.w3.org/2000/svg';
  function keySvg(s) {
    var svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('class', 'key-svg'); svg.setAttribute('viewBox', '0 0 28 10'); svg.setAttribute('aria-hidden', 'true');
    var line = document.createElementNS(NS, 'line');
    line.setAttribute('x1', '1'); line.setAttribute('y1', '5'); line.setAttribute('x2', '27'); line.setAttribute('y2', '5');
    line.setAttribute('class', 'line'); line.style.stroke = s.color;
    if (s.dash) line.setAttribute('stroke-dasharray', s.dash);
    svg.appendChild(line);
    var m = s.marker, g = null;
    if (m === 'circle') { g = document.createElementNS(NS, 'circle'); g.setAttribute('cx', '14'); g.setAttribute('cy', '5'); g.setAttribute('r', '3'); }
    else if (m === 'square') { g = document.createElementNS(NS, 'rect'); g.setAttribute('x', '11'); g.setAttribute('y', '2'); g.setAttribute('width', '6'); g.setAttribute('height', '6'); }
    else if (m === 'diamond') { g = document.createElementNS(NS, 'path'); g.setAttribute('d', 'M14,2 L17,5 L14,8 L11,5 Z'); }
    else if (m === 'triangle') { g = document.createElementNS(NS, 'path'); g.setAttribute('d', 'M14,2 L17,8 L11,8 Z'); }
    if (g) { g.setAttribute('class', 'mark'); g.style.fill = s.color; g.style.stroke = s.color; svg.appendChild(g); }
    return svg;
  }
  // Bind once per figure; re-run on a live re-render (`window.svBindCharts`).
  function bind(root) {
  root.querySelectorAll('figure.chart[data-chart]').forEach(function (fig) {
    if (fig.dataset.bound) return;
    fig.dataset.bound = '1';
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
        var key = stack ? document.createElement('span') : keySvg(s);
        if (stack) key.className = 'key ' + s.slot;
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
  }
  window.svBindCharts = bind;
  bind(document);
  var refresh = parseInt(document.body.getAttribute('data-refresh') || '0', 10);
  if (refresh > 0) {
    setInterval(function () { if (document.visibilityState === 'visible') location.reload(); }, refresh * 1000);
  }
})();
"#;

/// Theme bootstrap, the appearance picker, and the phone's view toggle.
/// Runs in `<head>` so a stored choice applies before first paint (no
/// flash). The world is a `data-theme` stamp on `<html>` (Foundry when
/// nothing is stored; each world is its own colour scheme); the tweaks
/// are inline custom properties on `<html>` — accent, radius, font,
/// density — plus `data-texture` / `data-motion` flags, so they compose
/// with every theme. Both live in localStorage (`sv-theme`, `sv-tweaks`),
/// wrapped in try/catch: private windows and blocked storage fall back to
/// Foundry. A link with `#theme=<id>&accent=…` imports a look once and
/// clears the fragment. The layout defines `window.svThemes` (the id
/// allow-list) before this runs; `light` / `dark` stored by the original
/// sun/moon toggle map onto Clinical / Foundry. The view toggle is
/// delegated because live pages swap `<main>` out from under it.
pub const THEME_SCRIPT: &str = r#"
(function () {
  var KEY = 'sv-theme', TWEAKS = 'sv-tweaks', VIEW = 'sv-view', root = document.documentElement;
  var THEMES = window.svThemes || [], DEFAULT = THEMES[0] || 'foundry';
  var ALIAS = { light: 'clinical', dark: 'foundry' };
  var FONTS = {
    mono: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    sans: 'system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif',
    serif: '"Iowan Old Style", "Palatino Linotype", Palatino, Georgia, "Times New Roman", serif',
    rounded: '"SF Pro Rounded", Nunito, "Varela Round", "Trebuchet MS", system-ui, sans-serif'
  };
  var DENSITY = { compact: 0.9, cozy: 1, relaxed: 1.12 };
  function stored(k) { try { return localStorage.getItem(k); } catch (_) { return null; } }
  function store(k, v) { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch (_) {} }
  function themeOf(id) { id = ALIAS[id] || id; return THEMES.indexOf(id) >= 0 ? id : DEFAULT; }
  // Accept only what the panel can produce; anything else is dropped.
  function cleanTweaks(o) {
    var t = {};
    if (!o || typeof o !== 'object') return t;
    if (/^#[0-9a-f]{6}$/i.test(o.accent || '')) t.accent = o.accent.toLowerCase();
    var r = typeof o.radius === 'string' ? parseFloat(o.radius) : o.radius;
    if (typeof r === 'number' && isFinite(r)) t.radius = Math.max(0, Math.min(24, Math.round(r)));
    if (DENSITY[o.density] && o.density !== 'cozy') t.density = o.density;
    if (FONTS[o.font]) t.font = o.font;
    if (o.texture === false || o.texture === '0' || o.texture === 0) t.texture = false;
    if (o.motion === false || o.motion === '0' || o.motion === 0) t.motion = false;
    return t;
  }
  var state = { theme: themeOf(stored(KEY)), tweaks: {} };
  try { state.tweaks = cleanTweaks(JSON.parse(stored(TWEAKS) || '{}')); } catch (_) { state.tweaks = {}; }
  (function fromHash() {
    var h = location.hash || '';
    if (!/^#(?:[^&]*&)*theme=/.test(h)) return;
    var params = {};
    h.slice(1).split('&').forEach(function (kv) {
      var i = kv.indexOf('=');
      if (i > 0) { try { params[decodeURIComponent(kv.slice(0, i))] = decodeURIComponent(kv.slice(i + 1)); } catch (_) {} }
    });
    if (params.accent && /^[0-9a-f]{6}$/i.test(params.accent)) params.accent = '#' + params.accent;
    state.theme = themeOf(params.theme);
    state.tweaks = cleanTweaks(params);
    persist();
    try { history.replaceState(null, '', location.pathname + location.search); } catch (_) {}
  })();
  function persist() {
    store(KEY, state.theme);
    store(TWEAKS, Object.keys(state.tweaks).length ? JSON.stringify(state.tweaks) : null);
  }
  function applyTheme(id) { root.setAttribute('data-theme', id); }
  function applyTweaks(t) {
    var st = root.style;
    if (t.accent) st.setProperty('--accent', t.accent); else st.removeProperty('--accent');
    if (typeof t.radius === 'number') {
      st.setProperty('--radius', t.radius + 'px');
      st.setProperty('--radius-lg', Math.round(t.radius * 1.5 + 2) + 'px');
    } else { st.removeProperty('--radius'); st.removeProperty('--radius-lg'); }
    if (FONTS[t.font]) { st.setProperty('--font', FONTS[t.font]); st.setProperty('--font-display', FONTS[t.font]); }
    else { st.removeProperty('--font'); st.removeProperty('--font-display'); }
    if (DENSITY[t.density]) st.setProperty('--density', String(DENSITY[t.density])); else st.removeProperty('--density');
    if (t.texture === false) root.setAttribute('data-texture', 'off'); else root.removeAttribute('data-texture');
    if (t.motion === false) root.setAttribute('data-motion', 'off'); else root.removeAttribute('data-motion');
  }
  function token(name) { return getComputedStyle(root).getPropertyValue(name).trim(); }
  // The browser chrome (address bar, status bar) follows the page.
  function paintChrome() {
    var bg = token('--bg');
    if (!bg) return;
    var metas = document.querySelectorAll('meta[name="theme-color"]');
    for (var i = 0; i < metas.length; i++) metas[i].setAttribute('content', bg);
  }
  function refresh() { applyTheme(state.theme); applyTweaks(state.tweaks); persist(); paintChrome(); }
  applyTheme(state.theme);
  applyTweaks(state.tweaks);
  if (stored(VIEW) === 'full') root.setAttribute('data-view', 'full');
  document.addEventListener('click', function (e) {
    var button = e.target && e.target.closest ? e.target.closest('#view-toggle') : null;
    if (!button) return;
    var full = root.getAttribute('data-view') === 'full';
    if (full) root.removeAttribute('data-view'); else root.setAttribute('data-view', 'full');
    store(VIEW, full ? 'glance' : 'full');
  });
  document.addEventListener('DOMContentLoaded', function () {
    paintChrome();
    var button = document.getElementById('theme-toggle'), dialog = document.getElementById('theme-picker');
    if (!button || !dialog) return;
    if (typeof dialog.showModal !== 'function') { button.hidden = true; return; }
    function $(id) { return dialog.querySelector('#' + id); }
    var cards = dialog.querySelectorAll('.theme-card');
    var accent = $('tw-accent'), accentReset = $('tw-accent-reset'), radius = $('tw-radius'), radiusOut = $('tw-radius-out');
    var density = $('tw-density'), font = $('tw-font'), texture = $('tw-texture'), motion = $('tw-motion');
    var json = $('tw-json'), paste = $('tw-import'), status = $('tw-status');
    function exported() {
      var o = { theme: state.theme }, t = state.tweaks;
      Object.keys(t).forEach(function (k) { o[k] = t[k]; });
      return o;
    }
    function shareLink() {
      var q = ['theme=' + state.theme], t = state.tweaks;
      if (t.accent) q.push('accent=' + t.accent.slice(1));
      if (typeof t.radius === 'number') q.push('radius=' + t.radius);
      if (t.density) q.push('density=' + t.density);
      if (t.font) q.push('font=' + t.font);
      if (t.texture === false) q.push('texture=0');
      if (t.motion === false) q.push('motion=0');
      return location.origin + location.pathname + '#' + q.join('&');
    }
    function sync() {
      for (var i = 0; i < cards.length; i++) cards[i].setAttribute('aria-pressed', String(cards[i].getAttribute('data-theme-id') === state.theme));
      var a = token('--accent');
      if (/^#[0-9a-f]{6}$/i.test(a)) accent.value = a.toLowerCase();
      var r = typeof state.tweaks.radius === 'number' ? state.tweaks.radius : parseFloat(token('--radius')) || 0;
      radius.value = r; radiusOut.textContent = r + ' px';
      density.value = state.tweaks.density || 'cozy';
      font.value = state.tweaks.font || 'theme';
      texture.checked = state.tweaks.texture !== false;
      motion.checked = state.tweaks.motion !== false;
      json.value = JSON.stringify(exported());
    }
    function commit() { refresh(); sync(); }
    function say(text) {
      status.textContent = text;
      clearTimeout(say.t); say.t = setTimeout(function () { status.textContent = ''; }, 1800);
    }
    function copy(text, what) {
      function done() { say(what + ' copied'); }
      function fallback() {
        json.value = text; json.focus(); json.select();
        try { document.execCommand('copy'); done(); } catch (_) { say('select and copy'); }
        sync();
      }
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, fallback);
      else fallback();
    }
    var i;
    for (i = 0; i < cards.length; i++) (function (card) {
      var id = card.getAttribute('data-theme-id');
      function tryOn() { applyTheme(id); paintChrome(); }
      function putBack() { applyTheme(state.theme); paintChrome(); }
      card.addEventListener('mouseenter', tryOn);
      card.addEventListener('focus', tryOn);
      card.addEventListener('mouseleave', putBack);
      card.addEventListener('blur', putBack);
      card.addEventListener('click', function () { state.theme = id; commit(); });
    })(cards[i]);
    accent.addEventListener('input', function () { state.tweaks.accent = accent.value; applyTweaks(state.tweaks); paintChrome(); });
    accent.addEventListener('change', commit);
    accentReset.addEventListener('click', function () { delete state.tweaks.accent; commit(); });
    radius.addEventListener('input', function () {
      state.tweaks.radius = parseInt(radius.value, 10); radiusOut.textContent = radius.value + ' px'; applyTweaks(state.tweaks);
    });
    radius.addEventListener('change', commit);
    density.addEventListener('change', function () { if (density.value === 'cozy') delete state.tweaks.density; else state.tweaks.density = density.value; commit(); });
    font.addEventListener('change', function () { if (font.value === 'theme') delete state.tweaks.font; else state.tweaks.font = font.value; commit(); });
    texture.addEventListener('change', function () { if (texture.checked) delete state.tweaks.texture; else state.tweaks.texture = false; commit(); });
    motion.addEventListener('change', function () { if (motion.checked) delete state.tweaks.motion; else state.tweaks.motion = false; commit(); });
    $('tw-copy-json').addEventListener('click', function () { copy(json.value, 'JSON'); });
    $('tw-copy-link').addEventListener('click', function () { copy(shareLink(), 'link'); });
    $('tw-apply').addEventListener('click', function () {
      var o; try { o = JSON.parse(paste.value); } catch (_) { say('not JSON'); return; }
      state.theme = themeOf(o && o.theme); state.tweaks = cleanTweaks(o); paste.value = ''; commit(); say('applied');
    });
    $('tw-reset').addEventListener('click', function () { state.tweaks = {}; commit(); say('theme defaults'); });
    $('tw-done').addEventListener('click', function () { dialog.close(); });
    $('tw-close').addEventListener('click', function () { dialog.close(); });
    dialog.addEventListener('click', function (e) { if (e.target === dialog) dialog.close(); });
    dialog.addEventListener('close', function () { applyTheme(state.theme); paintChrome(); });
    button.addEventListener('click', function () { sync(); dialog.showModal(); });
  });
})();
"#;

/// Seconds between reloads of the history pages (visible tab only).
const HISTORY_REFRESH_S: u32 = 60;

/// Which header tab a page belongs to (device pages sit under the fleet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// `/ui` and `/ui/devices/{id}`.
    Fleet,
    /// `/ui/live`.
    Live,
    /// `/ui/history`.
    History,
}

fn layout(nav: Nav, title: &str, subject: &str, mode: PageMode, body: Markup) -> Markup {
    let refresh = match mode {
        PageMode::Timed(secs) => Some(secs),
        PageMode::Rows | PageMode::Live => None,
    };
    let live = (mode == PageMode::Live).then_some("1");
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="dark light";
                meta name="theme-color" content="#0b0b0d" media="(prefers-color-scheme: dark)";
                meta name="theme-color" content="#e9e7e2" media="(prefers-color-scheme: light)";
                title { (title) " — supervictor fleet" }
                link rel="stylesheet" href=(assets::stylesheet_href());
                script { "window.svThemes=" (PreEscaped(themes::ids_json())) ";" (PreEscaped(THEME_SCRIPT)) }
            }
            body data-refresh=[refresh] data-live=[live] {
                header {
                    h1 { a href="/ui" { "supervictor fleet" } }
                    nav aria-label="pages" {
                        a .current[nav == Nav::Fleet] href="/ui" aria-current=[(nav == Nav::Fleet).then_some("page")] { "fleet" }
                        a .current[nav == Nav::Live] href="/ui/live" aria-current=[(nav == Nav::Live).then_some("page")] { "live" }
                        a .current[nav == Nav::History] href="/ui/history" aria-current=[(nav == Nav::History).then_some("page")] { "history" }
                    }
                    span #live-dot { "static" }
                    span .subject { (subject) }
                    button #theme-toggle type="button" aria-label="appearance: pick a world or make it yours" title="appearance" aria-haspopup="dialog" aria-controls="theme-picker" {
                        svg viewBox="0 0 24 24" aria-hidden="true" {
                            circle .sw-a cx="8" cy="9" r="5.2" {}
                            circle .sw-b cx="16" cy="9" r="5.2" fill-opacity="0.92" {}
                            circle .sw-c cx="12" cy="15.5" r="5.2" fill-opacity="0.92" {}
                        }
                    }
                }
                main {
                    (body)
                    // Phone only (CSS): lift the desktop layer when a glance
                    // turns into work. Labelled by CSS from `data-view`.
                    button #view-toggle type="button" aria-label="switch between the glance and everything" {
                        span .when-glance { "show everything" }
                        span .when-full { "back to the glance" }
                    }
                }
                (theme_picker())
                script { (PreEscaped(LIVE_SCRIPT)) }
                script { (PreEscaped(CHART_SCRIPT)) }
            }
        }
    }
}

/// The appearance dialog: six worlds, the tweak panel, and the share /
/// import box. Pure markup; THEME_SCRIPT wires it. Each card carries its
/// world's preview colours inline. No `<form>` on purpose — the live page
/// pauses re-renders while a form has focus, and the demo's form guard
/// would swallow a `method="dialog"` submit.
fn theme_picker() -> Markup {
    html! {
        dialog #theme-picker aria-labelledby="theme-picker-title" {
            div .picker {
                div .picker-head {
                    h2 #theme-picker-title { "Appearance" }
                    p { "Six worlds. Hover to try one on, click to keep it." }
                    button #tw-close .picker-close type="button" aria-label="close" { "×" }
                }
                ul .theme-grid role="list" {
                    @for theme in themes::THEMES {
                        li {
                            button .theme-card type="button" data-theme-id=(theme.id) data-scheme=(theme.scheme.css()) aria-pressed="false"
                                style=(format!(
                                    "--p-bg:{};--p-surface:{};--p-accent:{};--p-text:{}",
                                    theme.preview.bg, theme.preview.surface, theme.preview.accent, theme.preview.text,
                                )) {
                                span .theme-preview aria-hidden="true" { span .p-tile {} span .p-lines { i {} i {} i {} } }
                                span .theme-name { (theme.name) }
                                span .theme-for { (theme.audience) }
                                span .theme-note { (theme.tagline) }
                            }
                        }
                    }
                }
                fieldset .tweaks {
                    legend { "Make it yours" }
                    label .tweak for="tw-accent" {
                        span { "Accent" }
                        span .tweak-accent {
                            input #tw-accent type="color" value="#ff7a1a";
                            button #tw-accent-reset type="button" title="back to the theme's accent" { "theme's" }
                        }
                    }
                    label .tweak for="tw-radius" {
                        span { "Corners" output #tw-radius-out { "3 px" } }
                        input #tw-radius type="range" min="0" max="24" step="1" value="3";
                    }
                    label .tweak for="tw-density" {
                        span { "Density" }
                        select #tw-density {
                            option value="compact" { "compact" }
                            option value="cozy" selected { "cozy" }
                            option value="relaxed" { "relaxed" }
                        }
                    }
                    label .tweak for="tw-font" {
                        span { "Type" }
                        select #tw-font {
                            option value="theme" selected { "the theme's" }
                            option value="mono" { "monospace" }
                            option value="sans" { "sans" }
                            option value="serif" { "serif" }
                            option value="rounded" { "rounded" }
                        }
                    }
                    label .tweak-check for="tw-texture" { input #tw-texture type="checkbox" checked; "background art and grain" }
                    label .tweak-check for="tw-motion" { input #tw-motion type="checkbox" checked; "motion" }
                }
                details .share {
                    summary { "Share or save this look" }
                    div .share-body {
                        textarea #tw-json readonly aria-label="this look as JSON" {}
                        div .share-actions {
                            button #tw-copy-json type="button" { "copy JSON" }
                            button #tw-copy-link type="button" { "copy link" }
                            span #tw-status .muted role="status" aria-live="polite" {}
                        }
                        textarea #tw-import aria-label="paste a look as JSON" placeholder="paste a look's JSON here" {}
                        div .share-actions {
                            button #tw-apply type="button" { "apply pasted look" }
                        }
                    }
                }
                div .picker-foot {
                    button #tw-reset type="button" { "reset tweaks" }
                    button #tw-done .primary type="button" { "done" }
                }
            }
        }
    }
}

/// Aggregate fleet rows the way `fleet::summarize` aggregates a snapshot:
/// staleness and power buckets count ACTIVE devices only.
pub fn summarize_rows(rows: &[FleetRow]) -> FleetSummary {
    let mut summary = FleetSummary {
        total: rows.len(),
        ..FleetSummary::default()
    };
    for row in rows {
        if row.device.status == status::ACTIVE {
            summary.active += 1;
            match row.staleness {
                Staleness::Fresh => summary.fresh += 1,
                Staleness::Stale => summary.stale += 1,
                Staleness::Dark => summary.dark += 1,
                Staleness::Unknown => summary.never += 1,
            }
            match row.power {
                PowerState::On => summary.on += 1,
                PowerState::Off => summary.off += 1,
                PowerState::Unknown => {}
            }
            if row.device.power_threshold_ma.is_some() {
                summary.threshold_overrides += 1;
            }
        } else {
            summary.inactive += 1;
        }
    }
    summary
}

/// The one-line answer a phone asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Status class: `on`, `off`, `stale`, `dark` or `unknown`.
    pub class: &'static str,
    /// Headline — the fleet's worst condition.
    pub head: String,
    /// The counts behind it.
    pub detail: String,
}

/// Rank the fleet's state: silence outranks a stale reading, a stale
/// reading outranks a machine that is off, and "all on" needs every
/// active device to be on.
pub fn verdict(s: &FleetSummary) -> Verdict {
    let unknown_power = s.active.saturating_sub(s.on + s.off);
    let (class, head) = if s.active == 0 {
        ("unknown", "no active devices".to_string())
    } else if s.dark > 0 {
        ("dark", format!("{} dark", s.dark))
    } else if s.stale > 0 {
        ("stale", format!("{} stale", s.stale))
    } else if s.off > 0 {
        ("off", format!("{} off", s.off))
    } else if s.on == s.active {
        (
            "on",
            if s.on == 1 {
                "on".into()
            } else {
                format!("all {} on", s.on)
            },
        )
    } else if s.on > 0 {
        ("unknown", format!("{} on, {unknown_power} unknown", s.on))
    } else {
        ("unknown", "no readings".to_string())
    };
    let mut parts: Vec<String> = Vec::new();
    for (n, word) in [
        (s.on, "on"),
        (s.off, "off"),
        (s.stale, "stale"),
        (s.dark, "dark"),
        (s.never, "never heard"),
        (s.inactive, "inactive"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    let detail = if parts.is_empty() {
        format!("{} registered", s.total)
    } else {
        parts.join(" · ")
    };
    Verdict {
        class,
        head,
        detail,
    }
}

/// How long ago an RFC 3339 timestamp was, for a glance: `just now`,
/// `4 min ago`, `3 h ago`, `2 d ago`; `never` without one.
pub fn ago(last: Option<&str>, now_unix: u64) -> String {
    let Some(t) = last.and_then(parse_rfc3339_unix) else {
        return "never".into();
    };
    let d = now_unix.saturating_sub(t);
    if d < 60 {
        "just now".into()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86_400 {
        format!("{} h ago", d / 3600)
    } else {
        format!("{} d ago", d / 86_400)
    }
}

fn verdict_block(v: &Verdict) -> Markup {
    html! {
        section .verdict.(v.class) aria-label="fleet verdict" {
            span .verdict-head { (v.head) }
            span .verdict-detail { (v.detail) }
        }
    }
}

/// One meter for register `i` of a live device: the bar is level/255 in
/// the device's colour, the text is the value in the register's unit. The
/// raw 8-bit level only drives the bar width (`--lv`); the readout and the
/// ARIA range are in real units. `extra` is a second class (`hero-meter` /
/// `mini`) for the glance; the bank's meters carry none so the live script
/// finds them all by `.meter`.
fn meter(d: &live::LiveDevice, i: usize, r: &live::RegisterInfo, extra: &str) -> Markup {
    let level = d.latest().map(|s| s.l[i]).unwrap_or(0);
    let value = METRICS[i].dequantize(level).map(f64::from);
    let text = value.map(charts::fmt_value);
    html! {
        div class={ "meter" @if !extra.is_empty() { " " (extra) } } data-register=(i) style={ "--lv:" (level) } role="meter"
            aria-label={ (d.device_id) " " (r.label) } aria-valuemin=(r.lo) aria-valuemax=(r.hi)
            aria-valuenow=[value] aria-valuetext=[text.as_ref().map(|t| format!("{t} {}", r.unit))] {
            span .meter-bar.(d.style.color.class()) style=[d.style.color.custom_hex().map(|h| format!("--c:{h}"))] {}
            span .meter-value {
                @match text {
                    Some(t) => (t),
                    None => "–",
                }
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

/// What the history page plots beyond the fleet aggregates: every device's
/// drawing style, and which devices the viewer has picked.
pub struct Selection<'a> {
    /// Resolved style per device id (`crate::style::resolve`).
    pub styles: &'a HashMap<String, Resolved>,
    /// Device ids to plot, in fleet order; empty plots nothing. The fleet
    /// aggregates (power / status stacks) always cover every device.
    pub selected: &'a [String],
}

/// Percent-encode a query value (RFC 3986 unreserved bytes pass through).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `/ui/history?window=…&device=…&device=…` for a selection; an empty
/// selection carries one empty `device=` so the URL says "none" rather
/// than "unspecified" (which would restore the viewer's remembered set).
pub fn history_href<'a>(window: Window, devices: impl Iterator<Item = &'a str>) -> String {
    let mut href = format!("/ui/history?window={}", window.label());
    let mut any = false;
    for id in devices {
        any = true;
        href.push_str("&device=");
        href.push_str(&percent_encode(id));
    }
    if !any {
        href.push_str("&device=");
    }
    href
}

/// One chip per registered device: its key, its name, and a link that
/// toggles it in or out of the selection. Selected chips are highlighted in
/// the device's own colour (`--c`).
fn device_chips(history: &History, window: Window, selection: &Selection<'_>) -> Markup {
    html! {
        nav .devices aria-label="devices to plot" {
            span .filters-label { "devices" }
            @for d in &history.devices {
                @let on = selection.selected.contains(&d.device_id);
                @let style = selection.styles.get(&d.device_id).cloned().unwrap_or_else(|| Resolved::slot(SLOTS as usize));
                @let toggled: Vec<&str> = if on {
                    selection.selected.iter().map(String::as_str).filter(|id| *id != d.device_id).collect()
                } else {
                    selection.selected.iter().map(String::as_str).chain(std::iter::once(d.device_id.as_str())).collect()
                };
                a .chip.on[on] href=(history_href(window, toggled.into_iter())) style={ "--c:" (style.color.css()) }
                    role="button" aria-pressed=(if on { "true" } else { "false" }) data-device=(d.device_id) {
                    (charts::key(&style)) (d.device_id)
                }
            }
            @if history.devices.is_empty() {
                span .muted { "no registered devices" }
            }
        }
    }
}

/// `GET /ui/history` — status timeline, headline counts, and one chart per
/// metric for the selected devices.
pub fn history_page(
    history: &History,
    summary: &FleetSummary,
    window: Window,
    subject: &str,
    selection: &Selection<'_>,
) -> Markup {
    let times: Vec<u64> = history.timeline.iter().map(|p| p.t).collect();
    let uplinks: usize = history.devices.iter().map(|d| d.uplinks).sum();
    // The plotted devices, in fleet order, each with its column index.
    let plotted: Vec<(usize, &DeviceSeries)> = history
        .devices
        .iter()
        .enumerate()
        .filter(|(_, d)| selection.selected.contains(&d.device_id))
        .collect();
    let style_of = |d: &DeviceSeries| {
        selection
            .styles
            .get(&d.device_id)
            .cloned()
            .unwrap_or_else(|| Resolved::slot(SLOTS as usize))
    };
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
        Nav::History,
        "history",
        subject,
        PageMode::for_window(window),
        html! {
            (window_filters("/ui/history", window))
            @if history.truncated {
                p .banner { "row cap reached: the most recent part of this window is not shown — pick a shorter window" }
            }
            // Phone: the verdict and the two fleet-wide stacks, drawn narrow.
            section .glance {
                (verdict_block(&verdict(summary)))
                div .charts-glance {
                    (charts::power_chart_in(Frame::NARROW, "power-glance", &history.power, history.threshold_ma, window))
                    (charts::status_chart_in(Frame::NARROW, "status-glance", &history.timeline, window))
                }
            }
            div .deep {
            (device_chips(history, window, selection))
            section .tiles {
                (charts::stat_tile("active devices", &summary.active.to_string(), &format!("{} registered, {} inactive", summary.total, summary.inactive), ""))
                // The tiles judge each device against its own threshold when
                // one is set; the fleet default alone would misdescribe them.
                @let (on_note, off_note) = if summary.threshold_overrides > 0 {
                    (format!("per-device thresholds — {} of {} set, fleet default {} mA", summary.threshold_overrides, summary.active, history.threshold_ma),
                     "below their thresholds".to_string())
                } else {
                    (format!("current ≥ {} mA", history.threshold_ma), format!("current < {} mA", history.threshold_ma))
                };
                (charts::stat_tile("powered on", &summary.on.to_string(), &on_note, "on"))
                (charts::stat_tile("powered off", &summary.off.to_string(), &off_note, "off"))
                (charts::stat_tile("fresh", &summary.fresh.to_string(), "heard from in the last 15 min", "fresh"))
                (charts::stat_tile("stale", &summary.stale.to_string(), "quiet 15 min – 2 h", "stale"))
                (charts::stat_tile("dark", &summary.dark.to_string(), "silent over 2 h", "dark"))
                (charts::stat_tile("never", &summary.never.to_string(), "no uplink on record", ""))
                (charts::stat_tile("uplinks", &uplinks.to_string(), &format!("in the last {}", window.label()), ""))
            }
            section .charts {
                @let threshold_label = format!("on ≥ {} mA", history.threshold_ma);
                @if plotted.is_empty() {
                    p .hint .wide { "no devices plotted — click a device above to add its trace" }
                } @else {
                    @for (m, metric) in METRICS.iter().enumerate().take(1) {
                        @let series: Vec<Series<'_>> = plotted.iter().map(|(col, d)| Series {
                            label: &d.device_id,
                            style: style_of(d),
                            values: &columns[*col][m],
                        }).collect();
                        div .wide { (charts::line_chart(metric.key, metric.label, metric.unit, &times, &series, window,
                            Some(Reference { value: history.threshold_ma as f64, label: &threshold_label }))) }
                    }
                }
                div .wide { (charts::power_chart("power", &history.power, history.threshold_ma, window)) }
                div .wide { (charts::status_chart("status", &history.timeline, window)) }
                @if !plotted.is_empty() {
                    @for (m, metric) in METRICS.iter().enumerate().skip(1) {
                        @let series: Vec<Series<'_>> = plotted.iter().map(|(col, d)| Series {
                            label: &d.device_id,
                            style: style_of(d),
                            values: &columns[*col][m],
                        }).collect();
                        div { (charts::line_chart(metric.key, metric.label, metric.unit, &times, &series, window, None)) }
                    }
                }
            }
            }
        },
    )
}

/// Spans the live strips can show, seconds (client-side; the seed and the
/// poll snapshot always cover `live::WINDOW_S`).
pub const LIVE_SPANS: [u64; 3] = [30, 60, 120];
/// Span selected on load.
pub const LIVE_DEFAULT_SPAN: u64 = 60;

/// `GET /ui/live` — register bank (one meter per device per register,
/// newest sample: the bar is level/255, the text is the value in the
/// column's unit; the raw level is never shown) and one scrolling strip per
/// register.
pub fn live_page(snapshot: &live::Snapshot, subject: &str) -> Markup {
    let data = serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".into());
    let samples: usize = snapshot.devices.iter().map(|d| d.samples.len()).sum();
    layout(
        Nav::Live,
        "live",
        subject,
        PageMode::Rows,
        html! {
            div #live data-live=(data) data-poll="/ui/live.json" {
                // Phone: one card per device — current as the big meter,
                // the other registers as small readouts. Same `.meter`
                // markup as the bank, so the live script updates both.
                section .glance {
                    @if snapshot.devices.is_empty() {
                        p .hint { "no registered devices" }
                    } @else {
                        ul .live-cards {
                            @for d in &snapshot.devices {
                                li .live-card data-device=(d.device_id) {
                                    a .name href={ "/ui/devices/" (d.device_id) } { (charts::key(&d.style)) (d.device_id) }
                                    @if let Some(r) = snapshot.registers.first() {
                                        span .mini-label { (r.label) " (" (r.unit) ")" }
                                        (meter(d, 0, r, "hero-meter"))
                                    }
                                    div .minis {
                                        @for (i, r) in snapshot.registers.iter().enumerate().skip(1) {
                                            div .mini {
                                                span .mini-label { (r.label) " (" (r.unit) ")" }
                                                (meter(d, i, r, "mini"))
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div .deep {
                section .tiles {
                    (charts::stat_tile("devices", &snapshot.devices.len().to_string(), "registered", ""))
                    (charts::stat_tile("samples", &samples.to_string(), &format!("in the last {} s", snapshot.window_s), ""))
                    (charts::stat_tile("resolution", "8-bit", "1 byte per register per sample", ""))
                }
                section .bank aria-label="register bank" {
                    table {
                        thead {
                            tr {
                                th { "device" }
                                @for r in &snapshot.registers {
                                    th { (r.label) " " span .unit { "(" (r.unit) ")" } }
                                }
                            }
                        }
                        tbody {
                            @for d in &snapshot.devices {
                                tr data-device=(d.device_id) {
                                    td { (charts::key(&d.style)) a href={ "/ui/devices/" (d.device_id) } { (d.device_id) } }
                                    @for (i, r) in snapshot.registers.iter().enumerate() {
                                        td { (meter(d, i, r, "")) }
                                    }
                                }
                            }
                        }
                    }
                    @if snapshot.devices.is_empty() {
                        p { "no registered devices" }
                    }
                }
                }
                nav .filters aria-label="strip span" {
                    span .filters-label { "span" }
                    @for s in LIVE_SPANS {
                        button type="button" .active[s == LIVE_DEFAULT_SPAN] data-span=(s) { (s) " s" }
                    }
                }
                @if snapshot.devices.len() >= 2 {
                    ul .legend {
                        @for d in &snapshot.devices {
                            li { (charts::key(&d.style)) (d.device_id) }
                        }
                    }
                }
                // The first register's strip is the glance; the rest are deep.
                section .strips {
                    @for (i, r) in snapshot.registers.iter().enumerate() {
                        figure .strip.wide[i == 0].deep[i > 0] data-register=(i) {
                            figcaption { (r.label) " " span .unit { "(" (r.unit) ")" } }
                            canvas width="640" height="150" role="img" aria-label={ "live " (r.label) ", last " (LIVE_DEFAULT_SPAN) " seconds" } {}
                        }
                    }
                }
                script { (PreEscaped(LIVE_PAGE_SCRIPT)) }
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
    /// How the device is drawn (colour, dash, marker), resolved.
    pub style: Resolved,
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

/// `GET /ui` — fleet table plus the register form; on a phone, the verdict
/// and one tappable row per device. `now_unix` dates the glance's "ago".
pub fn fleet_page(rows: &[FleetRow], csrf: &str, subject: &str, now_unix: u64) -> Markup {
    layout(
        Nav::Fleet,
        "fleet",
        subject,
        PageMode::Rows,
        html! {
            section .glance {
                (verdict_block(&verdict(&summarize_rows(rows))))
                @if rows.is_empty() {
                    p .hint { "no devices registered" }
                } @else {
                    ul .glance-list {
                        @for row in rows {
                            @let active = row.device.status == status::ACTIVE;
                            li .inactive[!active] data-device=(row.device.device_id) data-threshold=(row.threshold_ma) data-min-on=(row.device.effective_min_on_s()) {
                                a .glance-row href={ "/ui/devices/" (row.device.device_id) } {
                                    span .name { (charts::key(&row.style)) (row.device.device_id) }
                                    span .pow {
                                        @if active {
                                            span .badge.power.(row.power.label()) { (row.power.label()) }
                                            " " span .current-inline.current-now { @if let Some(c) = row.current { (c) " mA" } }
                                        } @else {
                                            span .badge.inactive { "inactive" }
                                        }
                                    }
                                    span .when.(row.staleness.label()) { (ago(row.last_uplink.as_deref(), now_unix)) }
                                }
                            }
                        }
                    }
                }
            }
            div .deep {
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
                        tr data-device=(row.device.device_id) data-threshold=(row.threshold_ma) data-min-on=(row.device.effective_min_on_s()) {
                            td { (charts::key(&row.style)) a href={ "/ui/devices/" (row.device.device_id) } { (row.device.device_id) } }
                            td { span .badge.(row.device.status) { (row.device.status) } }
                            td {
                                span .badge.power.(row.power.label()) { (row.power.label()) }
                                " " span .current-inline.current-now { @if let Some(c) = row.current { (c) " mA" } }
                                @if let Some(t) = row.device.power_threshold_ma { " " span .current-inline title="per-device power threshold" { "≥ " (t) } }
                                @if let Some(m) = row.device.min_on_s { " " span .current-inline.min-on title="minimum on-time: current must hold above the threshold this long before the device counts as producing" { "for " (m) " s" } }
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
            }
        },
    )
}

/// `GET /ui/devices/{id}` — detail: metric charts over the window, then
/// the most recent raw uplinks. On a phone: is it on, how much current,
/// when did it last speak, and the current trace drawn narrow. `now_unix`
/// dates the glance's "ago".
pub fn device_page(
    device: &DeviceRecord,
    history: &DeviceHistory<'_>,
    uplinks: &[UplinkRecord],
    csrf: &str,
    subject: &str,
    now_unix: u64,
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
    let last_at = last.map(|u| u.received_at.as_str());
    let staleness = crate::fleet::staleness_of(last_at, now_unix);
    let threshold_label = format!("on ≥ {} mA", history.threshold_ma);
    layout(
        Nav::Fleet,
        &device.device_id,
        subject,
        PageMode::for_window(history.window),
        html! {
            h2 { (device.device_id) }
            section .glance {
                div .hero.(history.power.label()) {
                    span .hero-state { (history.power.label()) }
                    @if let Some(c) = history.current { span .hero-current { (c) " mA" } }
                    span .hero-note {
                        (threshold_label)
                        @if device.status != status::ACTIVE { " · " span .badge.(device.status) { (device.status) } }
                    }
                }
                dl .facts {
                    div { dt { "last uplink" } dd .when.(staleness.label()) { (ago(last_at, now_unix)) } }
                    div { dt { "uplinks" } dd { (history.series.uplinks) " in " (history.window.label()) } }
                    div { dt { "firmware" } dd { (fw) } }
                    div { dt { "owner" } dd { (device.owner_id) } }
                }
                (window_filters(&base, history.window))
                @if history.truncated {
                    p .banner { "row cap reached: the most recent part of this window is not shown — pick a shorter window" }
                }
                div .charts-glance {
                    @let series = [Series { label: &device.device_id, style: history.style.clone(), values: &columns[0] }];
                    (charts::line_chart_in(Frame::NARROW, "current-glance", METRICS[0].label, METRICS[0].unit, &times, &series, history.window,
                        Some(Reference { value: history.threshold_ma as f64, label: &threshold_label })))
                }
            }
            div .deep {
            dl .meta {
                dt { "status" }
                dd { span .badge.(device.status) { (device.status) } " " (status_form(device, csrf)) }
                dt { "power" }
                dd {
                    span .badge.power.(history.power.label()) { (history.power.label()) }
                    @if let Some(c) = history.current { " " span .current-inline { (c) " mA, threshold " (history.threshold_ma) " mA" } }
                    @if let Some(m) = device.min_on_s { " " span .current-inline.min-on { "producing after " (m) " s above it" } }
                }
                dt { "power threshold" }
                dd { (threshold_form(device, history, csrf)) }
                dt { "chart style" }
                dd { (style_form(device, &history.style, csrf)) }
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
                @for (m, metric) in METRICS.iter().enumerate() {
                    @let series = [Series { label: &device.device_id, style: history.style.clone(), values: &columns[m] }];
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
            }
        },
    )
}

/// Revoke/reactivate toggle for a device (CSRF-protected form post).
/// Set or clear the device's power judgement: the threshold override and
/// the minimum on-time. Each number box is prefilled with the stored value
/// (empty when the fleet default / the last reading alone applies);
/// submitting a box empty clears that part. Both are submitted together.
fn threshold_form(device: &DeviceRecord, history: &DeviceHistory<'_>, csrf: &str) -> Markup {
    let own = device.power_threshold_ma.map(|t| t.to_string());
    let own_min_on = device.min_on_s.map(|m| m.to_string());
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
            " "
            label title="producing, not just powered: current must hold above the threshold this long (a heater zone pulsing never qualifies)" {
                "for "
                input type="number" name="min_on_s" step="1" min="0" max=(crate::fleet::MAX_MIN_ON_S) size="5"
                    value=[own_min_on.as_deref()]
                    placeholder="0";
                " s"
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

/// Pick how the device is drawn: one of the eight palette slots or a custom
/// colour, a line pattern, a point marker. The current resolved style is
/// preselected; "fleet order" clears the stored colour.
fn style_form(device: &DeviceRecord, style: &Resolved, csrf: &str) -> Markup {
    let stored = device.style.color.as_deref().and_then(Color::parse);
    let custom = style.color.custom_hex();
    html! {
        form .inline.style-form method="post" action={ "/ui/devices/" (device.device_id) "/style" } {
            input type="hidden" name="csrf" value=(csrf);
            span .swatches role="radiogroup" aria-label="colour" {
                @for n in 0..SLOTS {
                    @let slot = Color::Slot(n);
                    @let checked = stored == Some(slot.clone());
                    label .swatch style={ "--c:" (slot.css()) } title={ "palette " (slot.token()) } {
                        input type="radio" name="color" value=(slot.token()) checked[checked];
                        span {}
                    }
                }
                label .swatch.swatch-custom title="custom colour" {
                    input type="radio" name="color" value="custom" checked[custom.is_some()];
                    input type="color" name="custom_color" value=(custom.unwrap_or("#888888")) aria-label="custom colour";
                }
                label .swatch.swatch-auto title="no stored colour: palette slot by registration order" {
                    input type="radio" name="color" value="" checked[stored.is_none() && custom.is_none()];
                    span { "auto" }
                }
            }
            " "
            select name="line_style" aria-label="line style" {
                @for line in LineStyle::ALL {
                    option value=(line.label()) selected[line == style.line] { (line.label()) }
                }
            }
            " "
            select name="marker" aria-label="point marker" {
                @for marker in Marker::ALL {
                    option value=(marker.label()) selected[marker == style.marker] {
                        @if marker == Marker::None { "no marker" } @else { (marker.label()) }
                    }
                }
            }
            " "
            button type="submit" { "set" }
            " " (charts::key(style))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_hrefs_repeat_the_device_key_and_encode_ids() {
        assert_eq!(
            history_href(Window::M5, ["a", "b,c", "d e/ü"].into_iter()),
            "/ui/history?window=5m&device=a&device=b%2Cc&device=d%20e%2F%C3%BC"
        );
        assert_eq!(
            history_href(Window::H1, std::iter::empty()),
            "/ui/history?window=1h&device=",
            "an empty selection is explicit"
        );
        assert_eq!(percent_encode("A-z_0.9~"), "A-z_0.9~");
        assert_eq!(percent_encode("<&>\"'"), "%3C%26%3E%22%27");
    }

    fn summary(active: usize, on: usize, off: usize, stale: usize, dark: usize) -> FleetSummary {
        FleetSummary {
            total: active + 1,
            active,
            inactive: 1,
            fresh: active.saturating_sub(stale + dark),
            stale,
            dark,
            never: 0,
            on,
            off,
            threshold_overrides: 0,
        }
    }

    #[test]
    fn verdict_ranks_silence_over_stale_over_off_and_needs_every_device_on() {
        let v = verdict(&summary(3, 3, 0, 0, 0));
        assert_eq!((v.class, v.head.as_str()), ("on", "all 3 on"));
        assert_eq!(v.detail, "3 on · 1 inactive");
        assert_eq!(verdict(&summary(1, 1, 0, 0, 0)).head, "on");
        let v = verdict(&summary(3, 2, 1, 0, 0));
        assert_eq!((v.class, v.head.as_str()), ("off", "1 off"));
        let v = verdict(&summary(3, 2, 0, 1, 0));
        assert_eq!((v.class, v.head.as_str()), ("stale", "1 stale"));
        let v = verdict(&summary(3, 1, 1, 1, 1));
        assert_eq!(
            (v.class, v.head.as_str()),
            ("dark", "1 dark"),
            "dark beats everything"
        );
        assert_eq!(v.detail, "1 on · 1 off · 1 stale · 1 dark · 1 inactive");
        // Fresh but no current reading: on is not claimed.
        let v = verdict(&summary(3, 2, 0, 0, 0));
        assert_eq!((v.class, v.head.as_str()), ("unknown", "2 on, 1 unknown"));
        let v = verdict(&summary(2, 0, 0, 0, 0));
        assert_eq!((v.class, v.head.as_str()), ("unknown", "no readings"));
        let v = verdict(&FleetSummary {
            total: 2,
            inactive: 2,
            ..FleetSummary::default()
        });
        assert_eq!((v.class, v.head.as_str()), ("unknown", "no active devices"));
        assert_eq!(v.detail, "2 inactive");
        let v = verdict(&FleetSummary::default());
        assert_eq!(v.detail, "0 registered");
    }

    #[test]
    fn ago_is_coarse_and_never_negative() {
        let now = 1_783_742_400; // 2026-07-11T04:00:00Z
        assert_eq!(ago(None, now), "never");
        assert_eq!(ago(Some("garbage"), now), "never");
        assert_eq!(ago(Some("2026-07-11T03:59:30+00:00"), now), "just now");
        assert_eq!(ago(Some("2026-07-11T03:55:00+00:00"), now), "5 min ago");
        assert_eq!(ago(Some("2026-07-11T01:10:00+00:00"), now), "2 h ago");
        assert_eq!(ago(Some("2026-07-01T04:00:00+00:00"), now), "10 d ago");
        assert_eq!(
            ago(Some("2026-07-11T04:05:00+00:00"), now),
            "just now",
            "a clock ahead of ours is not the future"
        );
    }
}
