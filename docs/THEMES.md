# Themes

The fleet dashboard ships six looks, each in a light and a dark mode,
instead of a light/dark switch. Each world was drawn for a kind of business
that runs machines on a current clamp; all twelve read the same data the
same way, so a device keeps its colour and `on` stays green whichever one
the viewer picks. Two buttons in the header: the sun/moon flips the current
world's mode, the three-swatch button opens the picker. In the picker,
hover a card to try a world on, click to keep it; the mode switch at the
top is auto (follow the OS), light or dark.

| id | drawn for | dark | light |
|---|---|---|---|
| `foundry` | manufacturing, machine shops, control rooms | the furnace: brushed steel, ember glow, hazard stripes, condensed type | the safety floor: concrete, chevrons, orange and black |
| `clinical` | hospitals, labs, pharma cold rooms | the night shift: glass cards over a dim cyan–violet mesh | the ward at noon: frosted glass over a drifting mesh, heavy rounded sans |
| `boardroom` | facilities, commercial real estate, energy managers | gilt on ink: gold rules, Didot numerals in cream | the magazine spread: paper grain, thick ink rules, red folio, small caps |
| `neon` | breweries, venues, hospitality | synthwave: striped sun, perspective grid floor, chrome wordmark, glow | Miami: pastel sherbet, hot pink and teal |
| `phosphor` | data centres, IT ops | the CRT: phosphor bloom, amber, scanlines, vignette, blinking cursor | the green-bar printout: banded paper, tractor-feed margins, ballpoint blue |
| `greenhouse` | farms, vertical growers, cold chain | the night garden: fireflies, moss, terracotta | the glasshouse: drifting foliage, italic serif wordmark, seed-tray corners |

Foundry is the default. Auto mode follows the OS and keeps following it; a
`light` or `dark` choice stored by the old toggle is read as a mode.

## Make it yours

Every tweak composes with every world and mode, lives in the viewer's
browser (`localStorage`), and never reaches the server:

- **Accent** — any colour; the theme's own is one click away.
- **Corners** — 0 to 24 px; the large radius follows at 1.5× + 2.
- **Density** — compact / cozy / relaxed (scales the root font size, so
  every rem-based padding follows).
- **Type** — the theme's faces, or monospace / sans / serif / rounded.
- **Background art and grain** and **motion** on or off. Motion off also
  stops the drifting backgrounds; the OS reduced-motion setting does too.

"Share or save this look" gives the look as JSON (paste it back into the
same box on another browser) or as a link:

```
https://<endpoint>/ui#theme=neon&mode=dark&accent=ff2d95&radius=14&density=relaxed&font=sans&texture=0&motion=0
```

Opening such a link applies and stores the look once, then clears the
fragment. Only `theme` is required; unknown ids fall back to Foundry,
`mode` other than light/dark means auto, bad values are dropped.

## Semantics that never move

| token | meaning |
|---|---|
| `--green` | powered on, fresh |
| `--red` | powered off, dark |
| `--warn` | stale, polling, banners |
| `--accent` | the brand: links, the current tab, thresholds |
| `--accent-2` | the brand's partner in gradients |
| `--s0`…`--s7` | device series slots, assigned by registration order |

The series slots are the two validated steps of the reference palette,
chosen by mode (dark steps in dark mode, light steps in light mode), never
by world: validated contrast on every chart surface, and a device's colour
survives a theme switch. Chart figures keep the solid `--surface` the
palette was validated against even when tiles are glass. Three light-mode
slots sit under 3:1 and are relieved by the legend keys and the table view,
as before.

## The token vocabulary

`supervictor/endpoint/src/ui/themes.rs` lists every token in `TOKENS`;
each block must declare all of them. In groups:

- **Colour**: `--bg`, `--surface`, `--surface-2`, `--border`, `--text`,
  `--muted`, `--accent`, `--accent-2`, `--warn`, `--green`, `--blue`, `--red`.
- **Type**: `--font`, `--font-display`, `--display-weight`,
  `--display-tracking`, `--display-transform`, `--value-scale` (tile
  values, verdicts and heroes scale by it), `--label-case`, `--label-tracking`.
- **Shape**: `--radius`, `--radius-lg`.
- **Art**: `--bg-art` (background layers), `--art-size` (over 100 % to
  drift), `--art-attach` (`fixed` or `scroll`), `--art-anim` (`running`
  drifts), `--grain` (film-grain opacity, 0 to about 0.2), `--floor`,
  `--floor-h`, `--floor-transform` (a fixed strip along the bottom).
- **Cards**: `--card-bg` (solid, gradient or translucent), `--card-ring`
  (an extra box-shadow), `--card-blur` (backdrop blur for glass),
  `--card-shadow`, `--card-top`, `--card-left`.
- **Header**: `--header-bg`, `--header-blur`, `--header-rule`,
  `--header-rule-h`, `--wordmark` (gradient text; `none` for plain),
  `--wordmark-ink`, `--wordmark-style`, `--wordmark-glow` (a `filter`).
- **Glow**: `--glow` (text-shadow on verdicts, heroes, the live dot).

"None" for a shadow is `0 0 0 transparent`, because two shadows are joined
with a comma.

## Adding a theme

1. Two blocks in `supervictor/endpoint/src/ui/themes.css`,
   `:root[data-theme="<id>"][data-mode="dark"]` and `[data-mode="light"]`,
   each declaring `color-scheme` and every token above.
2. A row in `THEMES` in `themes.rs` with `light` and `dark` previews whose
   hexes equal each block's `--bg`, `--surface`, `--accent`, `--text`.
3. At most a few signature rules at the bottom of `themes.css` for what
   tokens cannot say (Phosphor's cursor and vignette, Neon's sun).

`cargo test -p supervictor-endpoint --test test_ui` checks all three stay in
step. Validate any new series colours with the dataviz palette validator
before changing the slots; the chrome can be anything, the data colours
cannot.
