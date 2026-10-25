//! Inline-SVG charts for the history views — pure functions from bucketed
//! data to `Markup`, no charting dependency, no client-side rendering.
//!
//! Conventions (shared with `style.css` and the hover script in
//! `views.rs`): one y axis per chart, hairline grid, 2 px lines, ≥ 8 px end
//! markers ringed in the surface colour, categorical colour per device
//! (`crate::style`: a palette slot `.s0`–`.s7` or a custom hex through
//! `--c`), optional dash pattern and per-point glyph so two devices that
//! share a colour still read apart, direct end labels for up to four
//! series, a legend whenever there are two or more, and a collapsed table
//! twin under every chart so no value is reachable by hover alone.

use maud::{html, Markup};

use crate::history::{PowerPoint, TimelinePoint, Window};
use crate::style::{Marker, Resolved};
use crate::time::format_rfc3339;

/// Drawing surface in viewBox units.
pub const W: f64 = 640.0;
/// Height of the plot area plus the x-axis band.
pub const H: f64 = 220.0;
const MT: f64 = 12.0;
const MB: f64 = 28.0;
const ML: f64 = 56.0;
const MR_PLAIN: f64 = 16.0;
/// Right margin when the series carry direct end labels.
const MR_LABELED: f64 = 104.0;
/// Up to this many series are direct-labelled at the line end.
pub const DIRECT_LABEL_MAX: usize = 4;
/// Categorical slots available; the 9th device onward shares the
/// de-emphasis slot instead of inventing a hue.
pub const SLOTS: usize = 8;
const GAP: f64 = 2.0;

/// A horizontal reference line (e.g. the power threshold) with a label.
pub struct Reference<'a> {
    /// Y value in the chart's unit.
    pub value: f64,
    /// Short label drawn above the line.
    pub label: &'a str,
}

/// One segment kind of a stacked chart: its label, CSS class and one
/// count per bucket.
pub struct Segment<'a> {
    /// Legend / tooltip label.
    pub label: &'a str,
    /// CSS class (`seg.<class>` and `key.<class>` must exist in style.css).
    pub class: &'a str,
    /// One count per bucket.
    pub values: Vec<usize>,
}

/// Missing buckets a live-window line is drawn straight across. The live
/// presets bucket at the uplink period, so timing jitter alone puts two
/// readings in one bucket and none in the next; one empty bucket there is
/// jitter, not an outage. Longer windows have ≥ 10 s buckets and bridge
/// nothing — a gap is a gap.
pub const LIVE_BRIDGE_BUCKETS: usize = 1;
/// Radius of the per-point marker glyph.
const MARK_R: f64 = 3.0;

/// One line on a chart.
pub struct Series<'a> {
    /// Legend / end label (escaped by maud).
    pub label: &'a str,
    /// Colour, dash pattern, marker.
    pub style: Resolved,
    /// One value per bucket; `None` breaks the line.
    pub values: &'a [Option<f64>],
}

/// One rendered line: SVG path data, every plotted point, the last one.
struct Trace {
    d: String,
    points: Vec<(f64, f64)>,
    end: Option<(f64, f64)>,
}

/// Inline `style` attribute carrying a custom colour (`--c:#rrggbb`), or
/// nothing for a palette slot (the class does the work).
fn inline_color(style: &Resolved) -> Option<String> {
    style.color.custom_hex().map(|hex| format!("--c:{hex}"))
}

/// The per-point glyph for `marker` centred on (`cx`, `cy`); nothing for
/// [`Marker::None`].
pub fn glyph(marker: Marker, cx: f64, cy: f64, r: f64, style: &Resolved) -> Markup {
    let class = style.color.class();
    let inline = inline_color(style);
    html! {
        @match marker {
            Marker::None => {},
            Marker::Circle => circle .mark.(class) style=[inline] cx=(fmt1(cx)) cy=(fmt1(cy)) r=(fmt1(r)) {},
            Marker::Square => rect .mark.(class) style=[inline] x=(fmt1(cx - r)) y=(fmt1(cy - r)) width=(fmt1(2.0 * r)) height=(fmt1(2.0 * r)) {},
            Marker::Diamond => path .mark.(class) style=[inline]
                d={ "M" (fmt1(cx)) "," (fmt1(cy - r)) " L" (fmt1(cx + r)) "," (fmt1(cy)) " L" (fmt1(cx)) "," (fmt1(cy + r)) " L" (fmt1(cx - r)) "," (fmt1(cy)) " Z" } {},
            Marker::Triangle => path .mark.(class) style=[inline]
                d={ "M" (fmt1(cx)) "," (fmt1(cy - r)) " L" (fmt1(cx + r)) "," (fmt1(cy + r)) " L" (fmt1(cx - r)) "," (fmt1(cy + r)) " Z" } {},
        }
    }
}

/// Legend / chip / table key for a style: a short line sample with its
/// dash pattern and marker, in its colour. Replaces a flat colour square so
/// two devices sharing a colour are told apart in the key too.
pub fn key(style: &Resolved) -> Markup {
    let class = style.color.class();
    let inline = inline_color(style);
    html! {
        svg .key-svg viewBox="0 0 28 10" aria-hidden="true" {
            line .line.(class).(style.line.label()) style=[inline.clone()] x1="1" y1="5" x2="27" y2="5" {}
            (glyph(style.marker, 14.0, 5.0, MARK_R, style))
        }
    }
}

/// CSS class for a slot.
pub fn slot_class(slot: usize) -> String {
    if slot < SLOTS {
        format!("s{slot}")
    } else {
        "s-other".to_string()
    }
}

/// A linear y axis with "nice" 1-2-5 ticks spanning the data.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    /// Bottom of the axis.
    pub lo: f64,
    /// Top of the axis.
    pub hi: f64,
    /// Tick positions, ascending, `lo..=hi`.
    pub ticks: Vec<f64>,
}

impl Axis {
    /// Build from the data extent. A flat series gets a band around its
    /// value; non-finite inputs are treated as absent.
    pub fn nice(min: f64, max: f64) -> Axis {
        let (mut min, mut max) = if min.is_finite() && max.is_finite() && min <= max {
            (min, max)
        } else {
            (0.0, 1.0)
        };
        if (max - min).abs() < f64::EPSILON {
            let pad = (min.abs() * 0.1).max(1.0);
            min -= pad;
            max += pad;
        }
        let raw = (max - min) / 4.0;
        let mag = 10f64.powf(raw.log10().floor());
        let norm = raw / mag;
        let step = if norm <= 1.0 {
            1.0
        } else if norm <= 2.0 {
            2.0
        } else if norm <= 5.0 {
            5.0
        } else {
            10.0
        } * mag;
        let lo = (min / step).floor() * step;
        let hi = (max / step).ceil() * step;
        let mut ticks = Vec::new();
        let mut t = lo;
        while t <= hi + step * 0.5 && ticks.len() < 12 {
            ticks.push(t);
            t += step;
        }
        Axis { lo, hi, ticks }
    }

    /// Integer-only axis from 0 for counts.
    pub fn counts(max: usize) -> Axis {
        let max = max.max(1) as f64;
        let step = if max <= 5.0 {
            1.0
        } else if max <= 10.0 {
            2.0
        } else {
            (max / 5.0).ceil()
        };
        let hi = (max / step).ceil() * step;
        let ticks = (0..)
            .map(|i| i as f64 * step)
            .take_while(|t| *t <= hi + 0.5)
            .collect();
        Axis { lo: 0.0, hi, ticks }
    }

    fn y(&self, v: f64, ph: f64) -> f64 {
        let span = (self.hi - self.lo).max(f64::EPSILON);
        MT + (self.hi - v) / span * ph
    }
}

/// Compact human number: `0.12`, `23.5`, `1240`, `96K`, `1.2M`.
pub fn fmt_value(v: f64) -> String {
    if !v.is_finite() {
        return "–".into();
    }
    let a = v.abs();
    if a >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if a >= 10_000.0 {
        format!("{:.0}K", v / 1_000.0)
    } else if a >= 100.0 || v.fract() == 0.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Axis-tick label for a bucket time: `HH:MM` up to a day, `MM-DD HH:MM` beyond.
pub fn time_label(t: u64, window: Window) -> String {
    let s = format_rfc3339(t, 0);
    match window {
        Window::D7 => format!("{} {}", &s[5..10], &s[11..16]),
        _ => s[11..16].to_string(),
    }
}

/// Full timestamp for tooltips and tables.
pub fn time_full(t: u64) -> String {
    let s = format_rfc3339(t, 0);
    format!("{} {}", &s[0..10], &s[11..19])
}

fn x_of(i: usize, n: usize, ml: f64, pw: f64) -> f64 {
    if n <= 1 {
        return ml;
    }
    ml + i as f64 * pw / (n - 1) as f64
}

/// Five x ticks across the buckets.
fn x_ticks(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|f| ((n - 1) as f64 * f).round() as usize)
        .collect()
}

/// Multi-series line chart with hover layer and table twin. `times` are
/// bucket starts; every `series.values` must be `times.len()` long.
pub fn line_chart(
    chart_id: &str,
    title: &str,
    unit: &str,
    times: &[u64],
    series: &[Series<'_>],
    window: Window,
    reference: Option<Reference<'_>>,
) -> Markup {
    let n = times.len();
    let labeled = !series.is_empty() && series.len() <= DIRECT_LABEL_MAX;
    let mr = if labeled { MR_LABELED } else { MR_PLAIN };
    let pw = W - ML - mr;
    let ph = H - MT - MB;

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut any = false;
    for s in series {
        for v in s.values.iter().flatten() {
            if v.is_finite() {
                min = min.min(*v);
                max = max.max(*v);
                any = true;
            }
        }
    }
    let reference = reference.filter(|r| r.value.is_finite());
    if let Some(r) = &reference {
        min = min.min(r.value);
        max = max.max(r.value);
    }
    let axis = if any || reference.is_some() {
        Axis::nice(min, max)
    } else {
        Axis::nice(0.0, 1.0)
    };

    let bridge = if window.is_live() {
        LIVE_BRIDGE_BUCKETS
    } else {
        0
    };
    let paths: Vec<Trace> = series
        .iter()
        .map(|s| {
            let mut d = String::new();
            // Missing buckets since the last plotted point; `None` before
            // the first. Up to `bridge` of them are drawn across.
            let mut gap: Option<usize> = None;
            let mut points = Vec::new();
            let mut end = None;
            for (i, v) in s.values.iter().enumerate().take(n) {
                match v {
                    Some(v) if v.is_finite() => {
                        let x = x_of(i, n, ML, pw);
                        let y = axis.y(*v, ph);
                        let joined = matches!(gap, Some(g) if g <= bridge);
                        d.push_str(if joined { "L" } else { "M" });
                        d.push_str(&format!("{x:.1},{y:.1} "));
                        gap = Some(0);
                        points.push((x, y));
                        end = Some((x, y));
                    }
                    _ => {
                        if let Some(g) = gap.as_mut() {
                            *g += 1;
                        }
                    }
                }
            }
            Trace { d, points, end }
        })
        .collect();

    let line_series: Vec<serde_json::Value> = series
        .iter()
        .map(|s| {
            let values: Vec<Option<f64>> = s.values.iter().take(n).copied().collect();
            serde_json::json!({
                "label": s.label,
                "slot": s.style.color.class(),
                "color": s.style.color.css(),
                "dash": s.style.dash(),
                "marker": s.style.marker.label(),
                "values": values,
            })
        })
        .collect();
    let data = serde_json::json!({
        "kind": "line",
        "unit": unit,
        "t": times,
        "ml": ML, "pw": pw, "w": W,
        "series": line_series,
    });

    html! {
        figure .chart #(chart_id) data-chart=(data.to_string()) {
            figcaption { (title) " " span .unit { "(" (unit) ")" } }
            svg viewBox={ "0 0 " (W) " " (H) } role="img" aria-label=(title) preserveAspectRatio="none" {
                @for tick in &axis.ticks {
                    line .grid x1=(ML) x2=(W - mr) y1=(fmt1(axis.y(*tick, ph))) y2=(fmt1(axis.y(*tick, ph))) {}
                    text .tick x=(ML - 8.0) y=(fmt1(axis.y(*tick, ph) + 3.5)) text-anchor="end" { (fmt_value(*tick)) }
                }
                @for i in x_ticks(n) {
                    text .tick x=(fmt1(x_of(i, n, ML, pw))) y=(H - 8.0) text-anchor=(x_anchor(i, n)) { (time_label(times[i], window)) }
                }
                @if !any {
                    text .empty x=(ML + pw / 2.0) y=(MT + ph / 2.0) text-anchor="middle" { "no data in window" }
                }
                @if let Some(r) = &reference {
                    line .threshold x1=(ML) x2=(W - mr) y1=(fmt1(axis.y(r.value, ph))) y2=(fmt1(axis.y(r.value, ph))) {}
                    text .threshold-label x=(ML + 4.0) y=(fmt1(axis.y(r.value, ph) - 4.0)) { (r.label) }
                }
                @for (idx, trace) in paths.iter().enumerate() {
                    @let style = &series[idx].style;
                    @if !trace.d.is_empty() {
                        path .line.(style.color.class()).(style.line.label()) style=[inline_color(style)] d=(trace.d) {}
                    }
                    @if style.marker != Marker::None {
                        @for (x, y) in &trace.points {
                            (glyph(style.marker, *x, *y, MARK_R, style))
                        }
                    }
                }
                @for (idx, trace) in paths.iter().enumerate() {
                    @let style = &series[idx].style;
                    @if let Some((x, y)) = trace.end {
                        circle .dot.(style.color.class()) style=[inline_color(style)] cx=(fmt1(x)) cy=(fmt1(y)) r="4" {}
                        @if labeled {
                            text .end-label x=(fmt1(x + 8.0)) y=(fmt1(y + 3.5)) { (series[idx].label) }
                        }
                    }
                }
                line .crosshair x1="0" x2="0" y1=(MT) y2=(MT + ph) {}
            }
            div .tooltip hidden {}
            @if series.len() >= 2 {
                ul .legend {
                    @for s in series {
                        li { (key(&s.style)) (s.label) }
                    }
                }
            }
            (table_twin(unit, times, series))
        }
    }
}

fn table_twin(unit: &str, times: &[u64], series: &[Series<'_>]) -> Markup {
    html! {
        details .table-view {
            summary { "table" }
            table {
                thead {
                    tr {
                        th { "bucket (UTC)" }
                        @for s in series { th { (s.label) " " (unit) } }
                    }
                }
                tbody {
                    @for (i, t) in times.iter().enumerate() {
                        @if series.iter().any(|s| s.values.get(i).copied().flatten().is_some()) {
                            tr {
                                td { (time_full(*t)) }
                                @for s in series {
                                    td { (s.values.get(i).copied().flatten().map(fmt_value).unwrap_or_else(|| "–".into())) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Stacked columns of active-device staleness per bucket: fresh at the
/// baseline, then stale, dark, never — status colours, 2 px surface gaps.
pub fn status_chart(chart_id: &str, timeline: &[TimelinePoint], window: Window) -> Markup {
    let times: Vec<u64> = timeline.iter().map(|p| p.t).collect();
    let seg = |label: &'static str, f: fn(&TimelinePoint) -> usize| Segment {
        label,
        class: label,
        values: timeline.iter().map(f).collect(),
    };
    stacked_chart(
        chart_id,
        "fleet status over time",
        "active devices",
        "no active devices",
        &times,
        &[
            seg("fresh", |p| p.fresh),
            seg("stale", |p| p.stale),
            seg("dark", |p| p.dark),
            seg("never", |p| p.never),
        ],
        window,
    )
}

/// Stacked columns of active-device power state per bucket: on at the
/// baseline, then off, then unknown.
pub fn power_chart(
    chart_id: &str,
    points: &[PowerPoint],
    threshold_ma: i64,
    window: Window,
) -> Markup {
    let times: Vec<u64> = points.iter().map(|p| p.t).collect();
    let note = format!("on = current ≥ {threshold_ma} mA");
    let seg = |label: &'static str, f: fn(&PowerPoint) -> usize| Segment {
        label,
        class: label,
        values: points.iter().map(f).collect(),
    };
    stacked_chart(
        chart_id,
        "power state over time",
        &note,
        "no active devices",
        &times,
        &[
            seg("on", |p| p.on),
            seg("off", |p| p.off),
            seg("unknown", |p| p.unknown),
        ],
        window,
    )
}

/// Stacked columns, one per bucket, segments bottom-up in `segments`
/// order with 2 px surface gaps; legend, hover data and table twin.
#[allow(clippy::too_many_arguments)]
pub fn stacked_chart(
    chart_id: &str,
    title: &str,
    note: &str,
    empty_text: &str,
    times: &[u64],
    segments: &[Segment<'_>],
    window: Window,
) -> Markup {
    let n = times.len();
    let mr = MR_PLAIN;
    let pw = W - ML - mr;
    let ph = H - MT - MB;
    let total = (0..n)
        .map(|i| {
            segments
                .iter()
                .map(|s| s.values.get(i).copied().unwrap_or(0))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let axis = Axis::counts(total);
    let colw = if n > 0 { pw / n as f64 } else { pw };
    let barw = (colw - GAP).max(1.0);
    let unit_h = ph / axis.hi.max(1.0);
    // (class, x, y, width, height) per drawn segment, stacked from the
    // baseline with a surface gap between neighbours.
    let mut rects: Vec<(&str, f64, f64, f64, f64)> = Vec::new();
    for i in 0..n {
        let x = ML + i as f64 * colw + GAP / 2.0;
        let mut base = MT + ph;
        for s in segments {
            let count = s.values.get(i).copied().unwrap_or(0);
            if count == 0 {
                continue;
            }
            let h = count as f64 * unit_h;
            rects.push((s.class, x, base - h + GAP / 2.0, barw, (h - GAP).max(0.5)));
            base -= h;
        }
    }
    let stack_series: Vec<serde_json::Value> = segments
        .iter()
        .map(|s| serde_json::json!({ "label": s.label, "slot": s.class, "values": s.values }))
        .collect();
    let data = serde_json::json!({
        "kind": "stack",
        "unit": "devices",
        "t": times,
        "ml": ML, "pw": pw, "w": W,
        "series": stack_series,
    });

    html! {
        figure .chart.status-chart #(chart_id) data-chart=(data.to_string()) {
            figcaption { (title) " " span .unit { "(" (note) ")" } }
            svg viewBox={ "0 0 " (W) " " (H) } role="img" aria-label=(title) preserveAspectRatio="none" {
                @for tick in &axis.ticks {
                    line .grid x1=(ML) x2=(W - mr) y1=(fmt1(axis.y(*tick, ph))) y2=(fmt1(axis.y(*tick, ph))) {}
                    text .tick x=(ML - 8.0) y=(fmt1(axis.y(*tick, ph) + 3.5)) text-anchor="end" { (fmt_value(*tick)) }
                }
                @for i in x_ticks(n) {
                    text .tick x=(fmt1(ML + (i as f64 + 0.5) * colw)) y=(H - 8.0) text-anchor="middle" { (time_label(times[i], window)) }
                }
                @for (class, x, y, w, h) in &rects {
                    rect .seg.(class) x=(fmt1(*x)) y=(fmt1(*y)) width=(fmt1(*w)) height=(fmt1(*h)) {}
                }
                @if total == 0 {
                    text .empty x=(ML + pw / 2.0) y=(MT + ph / 2.0) text-anchor="middle" { (empty_text) }
                }
                line .crosshair x1="0" x2="0" y1=(MT) y2=(MT + ph) {}
            }
            div .tooltip hidden {}
            ul .legend {
                @for s in segments {
                    li { span .key.(s.class) {} (s.label) }
                }
            }
            details .table-view {
                summary { "table" }
                table {
                    thead { tr { th { "bucket (UTC)" } @for s in segments { th { (s.label) } } } }
                    tbody {
                        @for (i, t) in times.iter().enumerate() {
                            tr {
                                td { (time_full(*t)) }
                                @for s in segments { td { (s.values.get(i).copied().unwrap_or(0)) } }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Headline number with a label and a one-line note.
pub fn stat_tile(label: &str, value: &str, note: &str, class: &str) -> Markup {
    html! {
        div .tile.(class) {
            span .tile-label { (label) }
            span .tile-value { (value) }
            span .tile-note { (note) }
        }
    }
}

fn fmt1(v: f64) -> String {
    format!("{v:.1}")
}

fn x_anchor(i: usize, n: usize) -> &'static str {
    if i == 0 {
        "start"
    } else if i + 1 == n {
        "end"
    } else {
        "middle"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_axis_spans_data_with_round_ticks() {
        let a = Axis::nice(23.4, 27.9);
        assert!(a.lo <= 23.4 && a.hi >= 27.9, "{a:?}");
        assert!(a.ticks.len() >= 3 && a.ticks.len() <= 12, "{a:?}");
        assert_eq!(a.ticks.first(), Some(&a.lo));
        let a = Axis::nice(-70.0, -60.0);
        assert!(a.lo <= -70.0 && a.hi >= -60.0, "{a:?}");
        let a = Axis::nice(1_234.0, 1_234.0);
        assert!(
            a.lo < 1_234.0 && a.hi > 1_234.0,
            "flat series gets a band: {a:?}"
        );
        let a = Axis::nice(f64::NAN, 3.0);
        assert_eq!(a, Axis::nice(0.0, 1.0));
        let a = Axis::nice(0.0, 0.0);
        assert!(a.lo < 0.0 && a.hi > 0.0, "{a:?}");
    }

    #[test]
    fn count_axis_is_integer_from_zero() {
        for max in [0, 1, 3, 6, 9, 12, 57] {
            let a = Axis::counts(max);
            assert_eq!(a.lo, 0.0);
            assert!(a.hi >= max as f64, "{max}: {a:?}");
            assert!(a.ticks.iter().all(|t| t.fract() == 0.0), "{a:?}");
        }
    }

    #[test]
    fn values_format_compactly() {
        assert_eq!(fmt_value(0.123), "0.12");
        assert_eq!(fmt_value(23.456), "23.5");
        assert_eq!(fmt_value(1240.0), "1240");
        assert_eq!(fmt_value(-61.0), "-61");
        assert_eq!(fmt_value(96_256.0), "96K");
        assert_eq!(fmt_value(1_250_000.0), "1.2M");
        assert_eq!(fmt_value(f64::NAN), "–");
    }

    #[test]
    fn time_labels_follow_the_window() {
        let t = 1_800_000_000; // 2027-01-15T08:00:00Z
        assert_eq!(time_label(t, Window::H1), "08:00");
        assert_eq!(time_label(t, Window::D7), "01-15 08:00");
        assert_eq!(time_full(t), "2027-01-15 08:00:00");
    }

    #[test]
    fn line_chart_breaks_on_gaps_and_labels_small_series() {
        let times: Vec<u64> = (0..5).map(|i| 1_800_000_000 + i * 60).collect();
        let a = [Some(1.0), Some(2.0), None, Some(4.0), Some(5.0)];
        let b = [None, None, Some(1.0), Some(1.5), None];
        let html = line_chart(
            "c",
            "current",
            "mA",
            &times,
            &[
                Series {
                    label: "<dev&1>",
                    style: Resolved::slot(0),
                    values: &a,
                },
                Series {
                    label: "dev-2",
                    style: Resolved::slot(9),
                    values: &b,
                },
            ],
            Window::H1,
            Some(Reference {
                value: 3.0,
                label: "on ≥ 3 mA",
            }),
        )
        .into_string();
        assert!(html.contains("class=\"threshold\""), "reference line drawn");
        assert!(html.contains("on ≥ 3 mA"));
        // Two path segments for `a` (gap at index 2) => two "M" commands:
        // a standard window bridges nothing.
        let a_path = html.split("class=\"line s0 solid\"").nth(1).unwrap();
        let d = a_path
            .split("d=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert_eq!(d.matches('M').count(), 2, "{d}");
        assert!(
            html.contains("class=\"line s-other solid\""),
            "9th slot folds to other"
        );
        assert!(html.contains("&lt;dev&amp;1&gt;"), "labels escaped");
        assert!(!html.contains("<dev&1>"));
        assert!(
            html.contains("class=\"end-label\""),
            "<= 4 series are direct-labelled"
        );
        assert!(
            html.contains("class=\"legend\""),
            ">= 2 series get a legend"
        );
        assert!(html.contains("class=\"table-view\""));
        assert!(html.contains("data-chart="));
    }

    #[test]
    fn single_series_has_no_legend_and_empty_chart_says_so() {
        let times = [1_800_000_000u64, 1_800_000_060];
        let v = [Some(1.0), Some(2.0)];
        let html = line_chart(
            "c",
            "t",
            "°C",
            &times,
            &[Series {
                label: "only",
                style: Resolved::slot(0),
                values: &v,
            }],
            Window::H1,
            None,
        )
        .into_string();
        assert!(!html.contains("class=\"legend\""));
        assert!(!html.contains("class=\"threshold\""));
        let none: [Option<f64>; 2] = [None, None];
        let html = line_chart(
            "c",
            "t",
            "°C",
            &times,
            &[Series {
                label: "only",
                style: Resolved::slot(0),
                values: &none,
            }],
            Window::H1,
            None,
        )
        .into_string();
        assert!(html.contains("no data in window"));
        assert!(!html.contains("<path"));
    }

    /// Live windows bucket at the uplink period, where timing jitter alone
    /// empties a bucket now and then: one missing bucket is drawn across,
    /// two break the line. Standard windows bridge nothing.
    #[test]
    fn live_windows_bridge_a_single_missing_bucket() {
        let times: Vec<u64> = (0..7).map(|i| 1_800_000_000 + i * 5).collect();
        let v = [Some(1.0), None, Some(2.0), None, None, Some(3.0), Some(4.0)];
        let path_of = |window: Window| {
            let html = line_chart(
                "c",
                "current",
                "mA",
                &times,
                &[Series {
                    label: "d",
                    style: Resolved::slot(0),
                    values: &v,
                }],
                window,
                None,
            )
            .into_string();
            html.split("class=\"line s0 solid\"")
                .nth(1)
                .unwrap()
                .split("d=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string()
        };
        let live = path_of(Window::M1);
        assert_eq!(
            live.matches('M').count(),
            2,
            "1-bucket gap bridged, 2-bucket gap kept: {live}"
        );
        let standard = path_of(Window::H1);
        assert_eq!(standard.matches('M').count(), 3, "{standard}");
    }

    /// Dash pattern and marker glyphs follow the series style; a custom
    /// colour rides on `--c`; the legend key shows the same line sample.
    #[test]
    fn series_style_draws_dashes_markers_and_custom_colours() {
        use crate::style::{Color, LineStyle};
        let times = [1_800_000_000u64, 1_800_000_060, 1_800_000_120];
        let a = [Some(1.0), Some(2.0), Some(3.0)];
        let b = [Some(3.0), None, Some(1.0)];
        let html = line_chart(
            "c",
            "t",
            "°C",
            &times,
            &[
                Series {
                    label: "dashed-circles",
                    style: Resolved {
                        color: Color::Slot(2),
                        line: LineStyle::Dashed,
                        marker: Marker::Circle,
                    },
                    values: &a,
                },
                Series {
                    label: "hex-diamonds",
                    style: Resolved {
                        color: Color::Hex("#123abc".into()),
                        line: LineStyle::Dotted,
                        marker: Marker::Diamond,
                    },
                    values: &b,
                },
            ],
            Window::H1,
            None,
        )
        .into_string();
        assert!(
            html.contains("<path class=\"line s2 dashed\" d=\""),
            "{html}"
        );
        assert_eq!(
            html.matches("<circle class=\"mark s2\"").count(),
            4,
            "one per point plus the legend key"
        );
        assert!(
            html.contains("<path class=\"line custom dotted\" style=\"--c:#123abc\""),
            "{html}"
        );
        assert_eq!(
            html.matches("<path class=\"mark custom\" style=\"--c:#123abc\"")
                .count(),
            3,
            "diamonds only where there is a value, plus the legend key"
        );
        assert!(
            html.contains("<circle class=\"dot custom\" style=\"--c:#123abc\""),
            "end dot in the custom colour"
        );
        // Legend keys are line samples with the pattern + glyph.
        let legend = html.split("class=\"legend\"").nth(1).unwrap();
        assert_eq!(legend.matches("class=\"key-svg\"").count(), 2);
        assert!(legend.contains("class=\"line s2 dashed\""));
        assert!(legend.contains("class=\"line custom dotted\" style=\"--c:#123abc\""));
        // Hover data carries the style for the tooltip key.
        assert!(html.contains("&quot;dash&quot;:&quot;8 5&quot;"));
        assert!(html.contains("&quot;dash&quot;:&quot;1.5 4&quot;"));
        assert!(html.contains("&quot;color&quot;:&quot;var(--s2)&quot;"));
        assert!(html.contains("&quot;marker&quot;:&quot;diamond&quot;"));
        // No glyphs for the default marker.
        let plain = key(&Resolved::slot(1)).into_string();
        assert!(!plain.contains("mark"), "{plain}");
        assert!(plain.contains("class=\"line s1 solid\""));
        for marker in [Marker::Square, Marker::Triangle] {
            let k = key(&Resolved {
                marker,
                ..Resolved::slot(1)
            })
            .into_string();
            assert!(k.contains("class=\"mark s1\""), "{marker:?}: {k}");
        }
    }

    #[test]
    fn status_chart_stacks_four_segments_with_gaps() {
        let tl: Vec<TimelinePoint> = (0..3)
            .map(|i| TimelinePoint {
                t: 1_800_000_000 + i * 40,
                fresh: 2,
                stale: 1,
                dark: 0,
                never: 1,
            })
            .collect();
        let html = status_chart("s", &tl, Window::H1).into_string();
        assert_eq!(html.matches("class=\"seg fresh\"").count(), 3);
        assert_eq!(html.matches("class=\"seg stale\"").count(), 3);
        assert_eq!(
            html.matches("class=\"seg dark\"").count(),
            0,
            "empty segments are not drawn"
        );
        assert_eq!(html.matches("class=\"seg never\"").count(), 3);
        assert!(html.contains("class=\"legend\""));
        let empty = status_chart("s", &[], Window::H1).into_string();
        assert!(empty.contains("no active devices"));
    }

    #[test]
    fn power_chart_stacks_on_off_unknown_and_names_the_threshold() {
        let pts: Vec<PowerPoint> = (0..2)
            .map(|i| PowerPoint {
                t: 1_800_000_000 + i * 40,
                on: 1,
                off: 1,
                unknown: 0,
            })
            .collect();
        let html = power_chart("power", &pts, 100, Window::H1).into_string();
        assert_eq!(html.matches("class=\"seg on\"").count(), 2);
        assert_eq!(html.matches("class=\"seg off\"").count(), 2);
        assert_eq!(html.matches("class=\"seg unknown\"").count(), 0);
        assert!(html.contains("on = current ≥ 100 mA"));
        assert!(html.contains("<th>on</th>") && html.contains("<th>unknown</th>"));
    }
}
