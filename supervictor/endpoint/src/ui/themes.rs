//! The dashboard's six worlds: one entry per `:root[data-theme="<id>"]`
//! block in `themes.css`. The picker renders from this list, so adding a
//! theme is a CSS block plus one row here; `tests/test_ui.rs` checks that
//! each row's preview colours match the CSS tokens of the same name and
//! that the block declares every token in [`TOKENS`].

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

/// Theme applied when nothing is stored (also the bare `:root` tokens and
/// the `dark` alias; `light` is an alias of `clinical`).
pub const DEFAULT_THEME: &str = "foundry";

/// Every token a theme block must declare. `style.css` is written against
/// these and nothing else.
pub const TOKENS: &[&str] = &[
    "--bg",
    "--bg-art",
    "--art-size",
    "--art-attach",
    "--art-anim",
    "--grain",
    "--floor",
    "--floor-h",
    "--floor-transform",
    "--surface",
    "--surface-2",
    "--card-bg",
    "--card-ring",
    "--card-blur",
    "--border",
    "--text",
    "--muted",
    "--accent",
    "--accent-2",
    "--warn",
    "--green",
    "--blue",
    "--red",
    "--font",
    "--font-display",
    "--display-weight",
    "--display-tracking",
    "--display-transform",
    "--value-scale",
    "--wordmark",
    "--wordmark-ink",
    "--wordmark-style",
    "--wordmark-glow",
    "--radius",
    "--radius-lg",
    "--label-case",
    "--label-tracking",
    "--card-shadow",
    "--card-top",
    "--card-left",
    "--header-bg",
    "--header-blur",
    "--header-rule",
    "--header-rule-h",
    "--glow",
    "--main-max",
    "--tile-min",
    "--chart-min",
];

/// Every theme, in picker order.
pub const THEMES: &[Theme] = &[
    Theme {
        id: "foundry",
        name: "Foundry",
        audience: "manufacturing · machine shops · control rooms",
        tagline: "The furnace: brushed steel, ember glow, hazard stripes, condensed type. Wide and dense, like the control-room wall.",
        scheme: Scheme::Dark,
        preview: Preview {
            bg: "#0b0b0d",
            surface: "#141417",
            accent: "#ff7a1a",
            text: "#ececee",
        },
    },
    Theme {
        id: "clinical",
        name: "Clinical",
        audience: "hospitals · labs · pharma cold rooms",
        tagline: "The ward at noon: frosted glass over a drifting cyan–violet mesh, big centred numbers, everything a pill.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#eef3f8",
            surface: "#ffffff",
            accent: "#0891b2",
            text: "#0b1626",
        },
    },
    Theme {
        id: "boardroom",
        name: "Boardroom",
        audience: "facilities · commercial real estate · energy managers",
        tagline: "The magazine spread: a centred masthead, Didot numerals, ink rules, paper grain, one narrow column.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#f3eee3",
            surface: "#faf6ee",
            accent: "#c8102e",
            text: "#161411",
        },
    },
    Theme {
        id: "neon",
        name: "Neon",
        audience: "breweries · venues · hospitality",
        tagline: "Synthwave: striped sun, perspective grid floor, chrome wordmark, centred stats, glow on everything live.",
        scheme: Scheme::Dark,
        preview: Preview {
            bg: "#0a0618",
            surface: "#15102e",
            accent: "#ff2d95",
            text: "#f3ecff",
        },
    },
    Theme {
        id: "phosphor",
        name: "Phosphor",
        audience: "data centres · IT ops · people who read logs",
        tagline: "The CRT: phosphor bloom, amber, scanlines, a vignette. Edge to edge, stats as one status line.",
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
        tagline: "The glasshouse: drifting foliage, an italic serif wordmark, seed-tray corners, roomy tiles, pill nav.",
        scheme: Scheme::Light,
        preview: Preview {
            bg: "#f4f3e8",
            surface: "#fbfaf2",
            accent: "#c2572b",
            text: "#1e2a1a",
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

    fn is_hex(hex: &str) -> bool {
        hex.len() == 7
            && hex.starts_with('#')
            && hex[1..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    }

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
                assert!(is_hex(hex), "{}: {hex}", t.id);
            }
        }
        assert_eq!(THEMES[0].id, DEFAULT_THEME, "the default is the first card");
        assert!(by_id(DEFAULT_THEME).is_some_and(|t| t.scheme == Scheme::Dark));
        assert!(
            by_id("light").is_none() && by_id("dark").is_none(),
            "aliases live in CSS and the script"
        );
        assert!(TOKENS.len() > 40 && TOKENS.iter().all(|t| t.starts_with("--")));
    }

    #[test]
    fn ids_json_is_a_json_array_of_every_id() {
        let parsed: Vec<String> = serde_json::from_str(&ids_json()).unwrap();
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(parsed, ids);
    }
}
