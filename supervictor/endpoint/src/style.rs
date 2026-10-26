//! Per-device chart style: colour, stroke pattern, point marker.
//!
//! A device record may carry a [`DeviceStyle`]; what the charts draw is
//! the [`Resolved`] style — stored parts, with the gaps filled by defaults.
//! Colour defaults positionally: devices without a stored colour take
//! palette slots in registration order, so a fleet that predates stored
//! colours keeps stable colours as new devices arrive. Registration assigns
//! the first slot the fleet is not already wearing, and stores it, so new
//! devices never collide with old ones.

use std::collections::HashMap;

use crate::error::AppError;
use crate::models::DeviceRecord;
pub use supervictor_common::models::{DeviceStyle, LineStyle, Marker};

/// Palette slots (`--s0`–`--s7` in the stylesheet, validated for both
/// surfaces). Positional fallback beyond them folds to the grey "other".
pub const SLOTS: u8 = 8;

/// A trace colour: a palette slot (theme-aware) or a custom hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Color {
    /// Palette slot; `>= SLOTS` is the de-emphasis grey.
    Slot(u8),
    /// Custom `#rrggbb`, lowercase.
    Hex(String),
}

impl Color {
    /// Parse a stored / submitted token: `s0`–`s7`, or `#rrggbb` in either
    /// case (normalised to lowercase). Anything else is `None`.
    pub fn parse(token: &str) -> Option<Color> {
        if let Some(n) = token.strip_prefix('s') {
            let n: u8 = n.parse().ok()?;
            // Exactly one digit, no sign, no leading zero games: "s07" is
            // not a slot.
            return (n < SLOTS && token.len() == 2).then_some(Color::Slot(n));
        }
        let hex = token.strip_prefix('#')?;
        (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Color::Hex(format!("#{}", hex.to_ascii_lowercase())))
    }

    /// Wire / storage token: `s3` or `#aabbcc`.
    pub fn token(&self) -> String {
        match self {
            Color::Slot(n) if *n < SLOTS => format!("s{n}"),
            Color::Slot(_) => "s-other".into(),
            Color::Hex(h) => h.clone(),
        }
    }

    /// CSS class on chart marks: the slot class, or `custom` (which reads
    /// the colour from the `--c` custom property set inline).
    pub fn class(&self) -> String {
        match self {
            Color::Slot(_) => self.token(),
            Color::Hex(_) => "custom".into(),
        }
    }

    /// CSS value for the colour, usable in a `--c:` declaration.
    pub fn css(&self) -> String {
        match self {
            Color::Slot(_) => format!("var(--{})", self.token()),
            Color::Hex(h) => h.clone(),
        }
    }

    /// The custom hex, if this is one.
    pub fn custom_hex(&self) -> Option<&str> {
        match self {
            Color::Hex(h) => Some(h),
            Color::Slot(_) => None,
        }
    }
}

/// The style a trace is drawn with once defaults are filled in.
///
/// Serialises as what a page script needs to draw it: `class` (`s3` /
/// `custom`), `color` (a CSS value: `var(--s3)` or the hex), `dash`
/// (`stroke-dasharray` or null) and `marker` (its token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Trace colour.
    pub color: Color,
    /// Stroke pattern.
    pub line: LineStyle,
    /// Per-point glyph.
    pub marker: Marker,
}

impl Resolved {
    /// Solid, unmarked trace in palette slot `n` (tests, demos, fallbacks).
    pub fn slot(n: usize) -> Resolved {
        Resolved {
            color: Color::Slot(u8::try_from(n).unwrap_or(u8::MAX)),
            line: LineStyle::Solid,
            marker: Marker::None,
        }
    }

    /// Stored parts over the positional colour `fallback`.
    pub fn of(style: &DeviceStyle, fallback: Color) -> Resolved {
        Resolved {
            color: style
                .color
                .as_deref()
                .and_then(Color::parse)
                .unwrap_or(fallback),
            line: style.line_style.unwrap_or_default(),
            marker: style.marker.unwrap_or_default(),
        }
    }

    /// `stroke-dasharray` for the line, `None` for solid.
    pub fn dash(&self) -> Option<&'static str> {
        dash_of(self.line)
    }
}

impl serde::Serialize for Resolved {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Resolved", 4)?;
        st.serialize_field("class", &self.color.class())?;
        st.serialize_field("color", &self.color.css())?;
        st.serialize_field("dash", &self.dash())?;
        st.serialize_field("marker", self.marker.label())?;
        st.end()
    }
}

/// `stroke-dasharray` for a line style (2 px stroke), `None` for solid.
pub fn dash_of(line: LineStyle) -> Option<&'static str> {
    match line {
        LineStyle::Solid => None,
        LineStyle::Dashed => Some("8 5"),
        LineStyle::Dotted => Some("1.5 4"),
        LineStyle::DashDot => Some("8 4 1.5 4"),
    }
}

/// Resolve every device's style. Devices without a stored colour take
/// palette slots in registration order (`created_at`, then id) counted
/// among themselves, so adding a device with a stored colour never shifts
/// a legacy device's colour.
pub fn resolve(devices: &[DeviceRecord]) -> HashMap<String, Resolved> {
    let mut legacy: Vec<&DeviceRecord> = devices
        .iter()
        .filter(|d| d.style.color.as_deref().and_then(Color::parse).is_none())
        .collect();
    legacy.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.device_id.cmp(&b.device_id))
    });
    let fallback: HashMap<&str, usize> = legacy
        .iter()
        .enumerate()
        .map(|(i, d)| (d.device_id.as_str(), i))
        .collect();
    devices
        .iter()
        .map(|d| {
            let slot = fallback.get(d.device_id.as_str()).copied().unwrap_or(0);
            (
                d.device_id.clone(),
                Resolved::of(&d.style, Resolved::slot(slot).color),
            )
        })
        .collect()
}

/// The colour to give a newly registered device: the first palette slot no
/// existing device resolves to, else the least-worn slot (lowest wins ties).
pub fn auto_color(devices: &[DeviceRecord]) -> Color {
    let mut worn = [0usize; SLOTS as usize];
    for style in resolve(devices).values() {
        if let Color::Slot(n) = style.color {
            if let Some(count) = worn.get_mut(n as usize) {
                *count += 1;
            }
        }
    }
    let (slot, _) = worn
        .iter()
        .enumerate()
        .min_by_key(|(i, count)| (**count, *i))
        .expect("palette has slots");
    Color::Slot(slot as u8)
}

/// Check a submitted style: the colour must be a palette slot or a hex
/// triplet (normalised to lowercase on the way through).
pub fn validate(style: DeviceStyle) -> Result<DeviceStyle, AppError> {
    let color = match style.color {
        None => None,
        Some(token) => Some(
            Color::parse(&token)
                .ok_or_else(|| AppError::InvalidPayload {
                    detail: "color must be a palette slot s0-s7 or #rrggbb".into(),
                    structured: Some(serde_json::json!({
                        "field": supervictor_common::fields::COLOR,
                        "allowed": "s0-s7 | #rrggbb",
                    })),
                })?
                .token(),
        ),
    };
    Ok(DeviceStyle { color, ..style })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, created: &str, color: Option<&str>) -> DeviceRecord {
        DeviceRecord {
            device_id: id.into(),
            owner_id: "o".into(),
            subject_dn: None,
            status: "active".into(),
            created_at: created.into(),
            power_threshold_ma: None,
            style: DeviceStyle {
                color: color.map(Into::into),
                ..Default::default()
            },
            desired_fw: None,
        }
    }

    #[test]
    fn colour_tokens_parse_strictly_and_normalise() {
        assert_eq!(Color::parse("s0"), Some(Color::Slot(0)));
        assert_eq!(Color::parse("s7"), Some(Color::Slot(7)));
        assert_eq!(Color::parse("#AaBbCc"), Some(Color::Hex("#aabbcc".into())));
        for bad in [
            "s8",
            "s-1",
            "s07",
            "s",
            "S1",
            "#abc",
            "#aabbccdd",
            "#ggg000",
            "aabbcc",
            "",
            "red",
            "#aabbc\u{0}",
            "s1 ",
            "var(--s1)",
        ] {
            assert_eq!(Color::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Color::Slot(3).token(), "s3");
        assert_eq!(Color::Slot(3).class(), "s3");
        assert_eq!(Color::Slot(3).css(), "var(--s3)");
        assert_eq!(Color::Slot(9).token(), "s-other");
        let hex = Color::Hex("#ff8800".into());
        assert_eq!(hex.token(), "#ff8800");
        assert_eq!(hex.class(), "custom");
        assert_eq!(hex.css(), "#ff8800");
        assert_eq!(hex.custom_hex(), Some("#ff8800"));
    }

    #[test]
    fn legacy_devices_take_slots_in_registration_order_among_themselves() {
        let devices = [
            device("zeta", "2026-01-01T00:00:00Z", None),
            device("alpha", "2026-02-01T00:00:00Z", None),
            device("mid", "2026-01-15T00:00:00Z", Some("s5")),
            device("hexy", "2026-01-10T00:00:00Z", Some("#123456")),
        ];
        let styles = resolve(&devices);
        assert_eq!(styles["zeta"].color, Color::Slot(0), "oldest legacy first");
        assert_eq!(styles["alpha"].color, Color::Slot(1));
        assert_eq!(styles["mid"].color, Color::Slot(5), "stored slot wins");
        assert_eq!(styles["hexy"].color, Color::Hex("#123456".into()));
        assert_eq!(styles["zeta"].line, LineStyle::Solid);
        assert_eq!(styles["zeta"].marker, Marker::None);
        // An unparseable stored colour behaves like none (corrupt row, not
        // a crash), and still counts positionally.
        let devices = [device("a", "2026-01-01T00:00:00Z", Some("purple"))];
        assert_eq!(resolve(&devices)["a"].color, Color::Slot(0));
    }

    #[test]
    fn auto_colour_avoids_worn_slots_then_wears_the_least_used() {
        assert_eq!(auto_color(&[]), Color::Slot(0));
        let devices = [
            device("a", "2026-01-01T00:00:00Z", None),
            device("b", "2026-01-02T00:00:00Z", Some("s1")),
            device("c", "2026-01-03T00:00:00Z", Some("#000000")),
        ];
        assert_eq!(auto_color(&devices), Color::Slot(2), "s0 legacy, s1 stored");
        let full: Vec<DeviceRecord> = (0..8)
            .map(|i| {
                device(
                    &format!("d{i}"),
                    "2026-01-01T00:00:00Z",
                    Some(&format!("s{i}")),
                )
            })
            .collect();
        assert_eq!(auto_color(&full), Color::Slot(0), "all worn once: lowest");
        let mut nine = full.clone();
        nine.push(device("d8", "2026-01-02T00:00:00Z", Some("s0")));
        assert_eq!(auto_color(&nine), Color::Slot(1), "s0 worn twice");
    }

    #[test]
    fn validate_normalises_hex_and_rejects_junk() {
        let ok = validate(DeviceStyle {
            color: Some("#ABCDEF".into()),
            line_style: Some(LineStyle::Dotted),
            marker: None,
        })
        .unwrap();
        assert_eq!(ok.color.as_deref(), Some("#abcdef"));
        assert_eq!(ok.line_style, Some(LineStyle::Dotted));
        assert!(validate(DeviceStyle::default()).unwrap().is_default());
        let err = validate(DeviceStyle {
            color: Some("<script>".into()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidPayload { .. }), "{err:?}");
    }

    #[test]
    fn resolved_serialises_for_page_scripts() {
        let slot = serde_json::to_value(Resolved::slot(3)).unwrap();
        assert_eq!(
            slot,
            serde_json::json!({ "class": "s3", "color": "var(--s3)", "dash": null, "marker": "none" })
        );
        let custom = serde_json::to_value(Resolved {
            color: Color::Hex("#ff8800".into()),
            line: LineStyle::Dotted,
            marker: Marker::Square,
        })
        .unwrap();
        assert_eq!(
            custom,
            serde_json::json!({ "class": "custom", "color": "#ff8800", "dash": "1.5 4", "marker": "square" })
        );
    }

    #[test]
    fn dashes_exist_for_every_non_solid_style() {
        assert_eq!(dash_of(LineStyle::Solid), None);
        for line in [LineStyle::Dashed, LineStyle::Dotted, LineStyle::DashDot] {
            assert!(dash_of(line).is_some(), "{line:?}");
        }
        assert_eq!(Resolved::slot(2).dash(), None);
    }
}
