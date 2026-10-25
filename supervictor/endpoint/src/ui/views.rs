//! maud templates — pure functions from data to `Markup`. All interpolated
//! values are auto-escaped by maud; `PreEscaped` is used only for the static
//! script constants below, which contain no user data (page data reaches
//! them through escaped `data-*` attributes).

use std::collections::HashMap;

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::fleet::FleetSummary;
pub use crate::fleet::{PowerState, Staleness};
use crate::history::{DeviceSeries, History, Window, METRICS};
use crate::live;
use crate::models::{DeviceRecord, UplinkRecord};
use crate::style::{Color, LineStyle, Marker, Resolved, SLOTS};
use crate::ui::charts::{self, slot_class, Reference, Series};
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
/// else is listening (the live strips). Where SSE doesn't survive the proxy
/// (Lambda / API GW buffer responses) a page that declares `data-poll` on
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
  // Fleet table: patch the device's row in place — last uplink, freshness,
  // current, and the power badge judged against the row's own threshold.
  function patchRow(ev) {
    var row = document.querySelector('tr[data-device="' + CSS.escape(ev.device_id) + '"]');
    if (!row) return;
    var last = row.querySelector('.last-uplink');
    if (last) last.textContent = ev.received_at;
    var badge = row.querySelector('.staleness');
    if (badge) { badge.textContent = 'fresh'; badge.className = 'badge staleness fresh'; }
    if (typeof ev.current === 'number') {
      var now = row.querySelector('.current-now');
      if (now) now.textContent = ev.current + ' mA';
      var threshold = parseInt(row.getAttribute('data-threshold'), 10);
      var power = row.querySelector('.badge.power');
      if (power && Number.isFinite(threshold)) {
        var state = ev.current >= threshold ? 'on' : 'off';
        power.textContent = state; power.className = 'badge power ' + state;
      }
    }
    flash(row);
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

/// The live page: a register bank of 8-bit meters and one scrolling strip
/// per register, drawn on canvas at 10 fps from the samples seeded in
/// `#live[data-live]` plus whatever `sv:uplink` / `sv:snapshot` deliver.
/// Levels are the y axis (1..=255 maps onto each register's fixed range,
/// 0 breaks the line), so the scale never jumps as data arrives; tick
/// labels are dequantised into real units. Colours come from the same
/// `--sN` slot tokens as the SVG charts, read at draw time so the theme
/// toggle applies immediately. All text goes in via `textContent`.
pub const LIVE_PAGE_SCRIPT: &str = r#"
(function () {
  var root = document.getElementById('live');
  if (!root) return;
  var seed; try { seed = JSON.parse(root.dataset.live); } catch (_) { return; }
  var REG = seed.registers, R = REG.length, KEEP_MS = seed.window_s * 1000;
  var skew = Date.now() - seed.now_ms;
  var span = 60, devices = {}, order = [];
  seed.devices.forEach(function (d) {
    devices[d.device_id] = { slot: d.slot, t: d.samples.map(function (s) { return s.t; }), l: d.samples.map(function (s) { return s.l; }) };
    order.push(d.device_id);
  });
  function css(name) { return getComputedStyle(document.documentElement).getPropertyValue(name).trim(); }
  function slotVar(slot) { return slot < 8 ? '--s' + slot : '--s-other'; }
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
  function push(id, t, l) {
    var d = devices[id];
    d.t.push(t); d.l.push(l);
    var cut = t - KEEP_MS - 10000;
    while (d.t.length && d.t[0] < cut) { d.t.shift(); d.l.shift(); }
  }
  function updateMeters(id) {
    var d = devices[id], last = d.l[d.l.length - 1];
    var row = root.querySelector('tr[data-device="' + CSS.escape(id) + '"]');
    if (!last || !row) return;
    row.querySelectorAll('.meter').forEach(function (m) {
      var r = parseInt(m.dataset.register, 10), lv = last[r] | 0, v = deq(r, lv);
      m.style.setProperty('--lv', lv);
      m.querySelector('.meter-value').textContent = v === null ? '–' : fmt(v);
      m.querySelector('.meter-level').textContent = lv ? String(lv) : '–';
      if (lv) m.setAttribute('aria-valuenow', lv); else m.removeAttribute('aria-valuenow');
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
        var d = devices[id];
        var color = colors[d.slot] || (colors[d.slot] = css(slotVar(d.slot)));
        ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = 2; ctx.lineJoin = 'round'; ctx.lineCap = 'round';
        ctx.beginPath();
        var up = true, lx, ly;
        for (var i = 0; i < d.t.length; i++) {
          if (d.t[i] < t0 - 2000) continue;
          var lv = d.l[i][r] | 0;
          if (!lv) { up = true; continue; }
          var px = x(Math.min(d.t[i], t1)), py = y(lv);
          if (up) ctx.moveTo(px, py); else ctx.lineTo(px, py);
          up = false; lx = px; ly = py;
        }
        ctx.stroke();
        if (lx !== undefined) {
          ctx.beginPath(); ctx.arc(lx, ly, 3.5, 0, Math.PI * 2); ctx.fill();
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
  setInterval(function () { if (document.visibilityState === 'visible') draw(); }, 100);
  draw();
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

fn layout(title: &str, subject: &str, mode: PageMode, body: Markup) -> Markup {
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
                meta name="theme-color" content="#0e0e10" media="(prefers-color-scheme: dark)";
                meta name="theme-color" content="#f4f4f5" media="(prefers-color-scheme: light)";
                title { (title) " — supervictor fleet" }
                link rel="stylesheet" href="/ui/assets/style.css";
                script { (PreEscaped(THEME_SCRIPT)) }
            }
            body data-refresh=[refresh] data-live=[live] {
                header {
                    h1 { a href="/ui" { "supervictor fleet" } }
                    nav {
                        a href="/ui" { "fleet" }
                        a href="/ui/live" { "live" }
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
        "history",
        subject,
        PageMode::for_window(window),
        html! {
            (window_filters("/ui/history", window))
            (device_chips(history, window, selection))
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
        },
    )
}

/// Spans the live strips can show, seconds (client-side; the seed and the
/// poll snapshot always cover `live::WINDOW_S`).
pub const LIVE_SPANS: [u64; 3] = [30, 60, 120];
/// Span selected on load.
pub const LIVE_DEFAULT_SPAN: u64 = 60;

/// `GET /ui/live` — register bank (one 8-bit meter per device per
/// register, newest sample: the bar is level/255, the text is the value in
/// the column's unit and the raw level) and one scrolling strip per register.
pub fn live_page(snapshot: &live::Snapshot, subject: &str) -> Markup {
    let data = serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".into());
    let samples: usize = snapshot.devices.iter().map(|d| d.samples.len()).sum();
    layout(
        "live",
        subject,
        PageMode::Rows,
        html! {
            div #live data-live=(data) data-poll="/ui/live.json" {
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
                                    td { span .key.(slot_class(d.slot)) {} a href={ "/ui/devices/" (d.device_id) } { (d.device_id) } }
                                    @for (i, r) in snapshot.registers.iter().enumerate() {
                                        @let level = d.latest().map(|s| s.l[i]).unwrap_or(0);
                                        @let value = METRICS[i].dequantize(level);
                                        td {
                                            div .meter data-register=(i) style={ "--lv:" (level) } role="meter"
                                                aria-label={ (d.device_id) " " (r.label) } aria-valuemin="1" aria-valuemax="255"
                                                aria-valuenow=[(level > 0).then_some(level)] {
                                                span .meter-bar.(slot_class(d.slot)) {}
                                                span .meter-value {
                                                    @match value {
                                                        Some(v) => (charts::fmt_value(f64::from(v))),
                                                        None => "–",
                                                    }
                                                }
                                                span .meter-level { @if level > 0 { (level) } @else { "–" } }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    @if snapshot.devices.is_empty() {
                        p { "no registered devices" }
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
                            li { span .key.line-key.(slot_class(d.slot)) {} (d.device_id) }
                        }
                    }
                }
                section .strips {
                    @for (i, r) in snapshot.registers.iter().enumerate() {
                        figure .strip.wide[i == 0] data-register=(i) {
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

/// `GET /ui` — fleet table plus the register form.
pub fn fleet_page(rows: &[FleetRow], csrf: &str, subject: &str) -> Markup {
    layout(
        "fleet",
        subject,
        PageMode::Rows,
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
                        tr data-device=(row.device.device_id) data-threshold=(row.threshold_ma) {
                            td { (charts::key(&row.style)) a href={ "/ui/devices/" (row.device.device_id) } { (row.device.device_id) } }
                            td { span .badge.(row.device.status) { (row.device.status) } }
                            td {
                                span .badge.power.(row.power.label()) { (row.power.label()) }
                                " " span .current-inline.current-now { @if let Some(c) = row.current { (c) " mA" } }
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
        PageMode::for_window(history.window),
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
                @let threshold_label = format!("on ≥ {} mA", history.threshold_ma);
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
}
