//! The dashboard's six worlds, each in two modes: one entry per pair of
//! `:root[data-theme="<id>"][data-mode="light|dark"]` blocks in
//! `themes.css`. The picker renders from this list, so adding a theme is
//! two CSS blocks plus one row here; `tests/test_ui.rs` checks that each
//! mode's preview colours match the CSS tokens of the same name and that
//! every block declares every token in [`TOKENS`].

/// Light or dark: the `data-mode` stamp and the `color-scheme` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    /// The `data-mode` / `color-scheme` value.
    pub fn css(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }
}

/// One world. `native` is the mode it was drawn in first (the picker's
/// tagline describes that one); both modes are complete designs.
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
    pub native: Mode,
    pub light: Preview,
    pub dark: Preview,
}

impl Theme {
    /// The preview for a mode.
    pub fn preview(&self, mode: Mode) -> &Preview {
        match mode {
            Mode::Light => &self.light,
            Mode::Dark => &self.dark,
        }
    }
}

/// Picker-card colours (hex, lowercase, as written in `themes.css`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preview {
    pub bg: &'static str,
    pub surface: &'static str,
    pub accent: &'static str,
    pub text: &'static str,
}

/// Theme applied when nothing is stored (also the bare `:root` tokens).
pub const DEFAULT_THEME: &str = "foundry";

/// Every token a theme block must declare, in the order the blocks list
/// them. `style.css` is written against these and nothing else.
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
];

/// Every theme, in picker order.
pub const THEMES: &[Theme] = &[
    Theme {
        id: "foundry",
        name: "Foundry",
        audience: "manufacturing · machine shops · control rooms",
        tagline: "Condensed industrial type, hazard stripes, a monospace readout. Dark is the furnace, light is the safety floor.",
        native: Mode::Dark,
        dark: Preview {
            bg: "#0b0b0d",
            surface: "#141417",
            accent: "#ff7a1a",
            text: "#ececee",
        },
        light: Preview {
            bg: "#e9e7e2",
            surface: "#f7f6f3",
            accent: "#e85d04",
            text: "#141416",
        },
    },
    Theme {
        id: "clinical",
        name: "Clinical",
        audience: "hospitals · labs · pharma cold rooms",
        tagline: "Frosted glass over a drifting cyan–violet mesh, heavy rounded sans. The ward at noon, or the night shift.",
        native: Mode::Light,
        light: Preview {
            bg: "#eef3f8",
            surface: "#ffffff",
            accent: "#0891b2",
            text: "#0b1626",
        },
        dark: Preview {
            bg: "#070b14",
            surface: "#0f1627",
            accent: "#22d3ee",
            text: "#e8eefc",
        },
    },
    Theme {
        id: "boardroom",
        name: "Boardroom",
        audience: "facilities · commercial real estate · energy managers",
        tagline: "Editorial: Didot numerals, thick ink rules, small caps, paper grain. A magazine spread, or gilt on ink.",
        native: Mode::Light,
        light: Preview {
            bg: "#f3eee3",
            surface: "#faf6ee",
            accent: "#c8102e",
            text: "#161411",
        },
        dark: Preview {
            bg: "#0f0e0c",
            surface: "#171512",
            accent: "#d4a73a",
            text: "#efe6d2",
        },
    },
    Theme {
        id: "neon",
        name: "Neon",
        audience: "breweries · venues · hospitality",
        tagline: "Synthwave: striped sun, perspective grid floor, chrome wordmark, glow. Light is Miami sherbet.",
        native: Mode::Dark,
        dark: Preview {
            bg: "#0a0618",
            surface: "#15102e",
            accent: "#ff2d95",
            text: "#f3ecff",
        },
        light: Preview {
            bg: "#fff1f6",
            surface: "#ffffff",
            accent: "#e6007e",
            text: "#2a1033",
        },
    },
    Theme {
        id: "phosphor",
        name: "Phosphor",
        audience: "data centres · IT ops · people who read logs",
        tagline: "The CRT: phosphor bloom, amber, scanlines, a vignette. Light is the green-bar printout with tractor-feed margins.",
        native: Mode::Dark,
        dark: Preview {
            bg: "#000000",
            surface: "#050b06",
            accent: "#ffb000",
            text: "#6ef28a",
        },
        light: Preview {
            bg: "#f4f7ee",
            surface: "#ffffff",
            accent: "#1d3f8a",
            text: "#16341c",
        },
    },
    Theme {
        id: "greenhouse",
        name: "Greenhouse",
        audience: "farms · vertical growers · cold chain",
        tagline: "Botanical: drifting foliage, an italic serif wordmark, seed-tray corners, terracotta. Dark is the night garden with fireflies.",
        native: Mode::Light,
        light: Preview {
            bg: "#f4f3e8",
            surface: "#fbfaf2",
            accent: "#c2572b",
            text: "#1e2a1a",
        },
        dark: Preview {
            bg: "#0c140e",
            surface: "#132018",
            accent: "#e07a4a",
            text: "#e8efdf",
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
    fn ids_are_unique_lowercase_and_six_with_two_previews_each() {
        assert_eq!(THEMES.len(), 6);
        let mut seen = std::collections::HashSet::new();
        for t in THEMES {
            assert!(
                t.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{}",
                t.id
            );
            assert!(seen.insert(t.id), "duplicate id {}", t.id);
            for p in [&t.light, &t.dark] {
                for hex in [p.bg, p.surface, p.accent, p.text] {
                    assert!(is_hex(hex), "{}: {hex}", t.id);
                }
            }
            assert_ne!(t.light.bg, t.dark.bg, "{}: modes must differ", t.id);
            assert_eq!(t.preview(t.native).bg, t.preview(t.native).bg);
        }
        assert_eq!(THEMES[0].id, DEFAULT_THEME, "the default is the first card");
        assert!(by_id("light").is_none() && by_id("auto").is_none());
        assert!(TOKENS.len() > 40 && TOKENS.iter().all(|t| t.starts_with("--")));
    }

    #[test]
    fn ids_json_is_a_json_array_of_every_id() {
        let parsed: Vec<String> = serde_json::from_str(&ids_json()).unwrap();
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(parsed, ids);
    }
}
