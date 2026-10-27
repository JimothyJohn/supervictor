//! The dashboard's six worlds: one entry per `:root[data-theme="<id>"]`
//! block in `themes.css`. The picker renders from this list, so adding a
//! theme is a CSS block plus one row here; `tests/test_ui.rs` checks that
//! each row's preview colours match the CSS tokens of the same name.

/// Whether a theme asks the browser for light or dark form controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    /// The `color-scheme` value the theme's CSS block declares.
    pub fn css(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }
}

/// One world. The `preview` colours paint the picker card and must equal
/// the theme's `--bg`, `--surface`, `--accent` and `--text` tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The `data-theme` stamp; lowercase, stable, part of the stored contract.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Who it was drawn for.
    pub audience: &'static str,
    /// One line on the look.
    pub tagline: &'static str,
    pub scheme: Scheme,
    pub preview: Preview,
}

/// Picker-card colours (hex, lowercase, as written in `themes.css`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preview {
    pub bg: &'static str,
    pub surface: &'static str,
    pub accent: &'static str,
    pub text: &'static str,
}

/// Theme applied when nothing is stored and the OS prefers dark (also the
/// bare `:root` tokens, and the `dark` alias).
pub const DEFAULT_DARK: &str = "foundry";
/// Theme applied when nothing is stored and the OS prefers light (the
/// `light` alias).
pub const DEFAULT_LIGHT: &str = "clinical";

/// Every theme, in picker order.
pub const THEMES: &[Theme] = &[
    Theme {
        id: "foundry",
        name: "Foundry",
        audience: "manufacturing · machine shops · control rooms",
        tagline: "Charcoal and molten orange, a monospace readout, a dot grid under the floor.",
        scheme: Scheme::Dark,
        preview: Preview {
            bg: "#0e0e10",
            surface: "#18181b",
            accent: "#f97316",
            text: "#e4e4e7",
        },
    },
    Theme {
        id: "clinical",
        name: "Clinical",
        audience: "hospitals · labs · pharma cold rooms",
        tagline: "White and teal, soft shadows, generous corners. Autoclaves and freezers at a glance.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#f1f5f8",
            surface: "#ffffff",
            accent: "#0e7490",
            text: "#0f1b2d",
        },
    },
    Theme {
        id: "boardroom",
        name: "Boardroom",
        audience: "facilities · commercial real estate · energy managers",
        tagline: "Ivory paper, navy ink, gold hairlines and serif numerals. Reads like the annual report.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#f4efe4",
            surface: "#fcf9f1",
            accent: "#1f3a5f",
            text: "#1c1a16",
        },
    },
    Theme {
        id: "neon",
        name: "Neon",
        audience: "breweries · venues · hospitality",
        tagline: "Deep violet, hot magenta and cyan, glow on the big numbers. After hours.",
        scheme: Scheme::Dark,
        preview: Preview {
            bg: "#0d0a1c",
            surface: "#171233",
            accent: "#ff2d95",
            text: "#efe9ff",
        },
    },
    Theme {
        id: "phosphor",
        name: "Phosphor",
        audience: "data centres · IT ops · people who read logs",
        tagline: "Black glass, green phosphor, amber accents, scanlines. No corners at all.",
        scheme: Scheme::Dark,
        preview: Preview {
            bg: "#000000",
            surface: "#050b06",
            accent: "#ffb000",
            text: "#6ef28a",
        },
    },
    Theme {
        id: "greenhouse",
        name: "Greenhouse",
        audience: "farms · vertical growers · cold chain",
        tagline: "Cream and sage, terracotta and harvest amber, rounded like a seed tray.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#f1f4ea",
            surface: "#fbfcf6",
            accent: "#b65a30",
            text: "#1f2a1c",
        },
    },
];

/// Look a theme up by its `data-theme` id.
pub fn by_id(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id == id)
}

/// Theme ids as a JSON array literal, for the bootstrap script's allow-list.
pub fn ids_json() -> String {
    let ids: Vec<String> = THEMES.iter().map(|t| format!("\"{}\"", t.id)).collect();
    format!("[{}]", ids.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_lowercase_and_six() {
        assert_eq!(THEMES.len(), 6);
        let mut seen = std::collections::HashSet::new();
        for t in THEMES {
            assert!(
                t.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{}",
                t.id
            );
            assert!(seen.insert(t.id), "duplicate id {}", t.id);
            for hex in [
                t.preview.bg,
                t.preview.surface,
                t.preview.accent,
                t.preview.text,
            ] {
                assert!(
                    hex.len() == 7
                        && hex.starts_with('#')
                        && hex[1..]
                            .chars()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                    "{}: {hex}",
                    t.id
                );
            }
        }
        assert!(by_id(DEFAULT_DARK).is_some_and(|t| t.scheme == Scheme::Dark));
        assert!(by_id(DEFAULT_LIGHT).is_some_and(|t| t.scheme == Scheme::Light));
        assert!(
            by_id("light").is_none(),
            "aliases live in CSS and the script, not the registry"
        );
    }

    #[test]
    fn ids_json_is_a_json_array_of_every_id() {
        let parsed: Vec<String> = serde_json::from_str(&ids_json()).unwrap();
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(parsed, ids);
    }
}
