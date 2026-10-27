# Themes

The fleet dashboard ships six looks instead of a light/dark switch. Each
world is its own colour scheme, drawn for a kind of business that runs
machines on a current clamp, and each arranges the same pieces a little
differently. All six read the same data the same way, so a device keeps
its colour and `on` stays green whichever world the viewer picks. The
three-swatch button in the header opens the picker: hover a card to try a
world on, click to keep it, and the panel below makes the look yours.

| id | drawn for | scheme | the look | the layout |
|---|---|---|---|---|
| `foundry` | manufacturing, machine shops, control rooms | dark | the furnace: brushed steel, ember glow, hazard stripes, condensed type | wide and dense, boxed nav segments, tight tiles |
| `clinical` | hospitals, labs, pharma cold rooms | light | the ward at noon: frosted glass over a drifting cyan–violet mesh, heavy rounded sans | roomy; the number first, centred, label beneath; pill nav |
| `boardroom` | facilities, commercial real estate, energy managers | light | the magazine spread: paper grain, thick ink rules, Didot numerals, red folio, small caps | a centred masthead, one narrow column, stats four across as bare figures |
| `neon` | breweries, venues, hospitality | dark | synthwave: striped sun, perspective grid floor, chrome wordmark, glow | centred stats, italic uppercase nav |
| `phosphor` | data centres, IT ops | dark | the CRT: phosphor bloom, amber, scanlines, vignette, blinking cursor | edge to edge; the stat tiles collapse into one status line; bracketed menu |
| `greenhouse` | farms, vertical growers, cold chain | light | the glasshouse: drifting foliage, italic serif wordmark, seed-tray corners, terracotta | fewer, bigger tiles; pill nav |

Foundry is the default. A `light` or `dark` choice stored by the original
toggle maps onto Clinical or Foundry.

## Make it yours

Every tweak composes with every world, lives in the viewer's browser
(`localStorage`), and never reaches the server:

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
https://<endpoint>/ui#theme=neon&accent=ff2d95&radius=14&density=relaxed&font=sans&texture=0&motion=0
```

Opening such a link applies and stores the look once, then clears the
fragment. Only `theme` is required; unknown ids fall back to Foundry, bad
values are dropped.

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
chosen by scheme (dark steps on dark worlds, light steps on light ones),
never by world: validated contrast on every chart surface, and a device's
colour survives a theme switch. Chart figures keep the solid `--surface`
the palette was validated against even when tiles are glass. Three
light-scheme slots sit under 3:1 and are relieved by the legend keys and
the table view, as before.

## The token vocabulary

`supervictor/endpoint/src/ui/themes.rs` lists every token in `TOKENS`;
each block must declare all of them. In groups:

- **Colour**: `--bg`, `--surface`, `--surface-2`, `--border`, `--text`,
  `--muted`, `--accent`, `--accent-2`, `--warn`, `--green`, `--blue`, `--red`.
- **Type**: `--font`, `--font-display`, `--display-weight`,
  `--display-tracking`, `--display-transform`, `--value-scale` (tile
  values, verdicts and heroes scale by it), `--label-case`, `--label-tracking`.
- **Shape**: `--radius`, `--radius-lg`.
- **Layout**: `--main-max` (page width), `--tile-min` (stat tile minimum
  width, so how many fit across), `--chart-min` (chart minimum width, so
  one column or several). Bigger rearrangements are signature rules.
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

1. A `:root[data-theme="<id>"]` block in
   `supervictor/endpoint/src/ui/themes.css` declaring `color-scheme` and
   every token above, and the id added to the dark or light series rule at
   the top of the file.
2. A row in `THEMES` in `themes.rs` whose preview hexes equal the block's
   `--bg`, `--surface`, `--accent`, `--text`.
3. Signature rules at the bottom of `themes.css`: a few for colour and
   texture (Phosphor's cursor and vignette, Neon's sun) and a few for
   desktop layout under `@media (min-width: 641px)`; phones keep the one
   glance layout.

`cargo test -p supervictor-endpoint --test test_ui` checks all of it stays
in step. Validate any new series colours with the dataviz palette validator
before changing the slots; the chrome can be anything, the data colours
cannot.

## Cache

Pages link the stylesheet as `/ui/assets/style.css?v=<content hash>` and
the asset is served immutable, so a new build is never read against a
browser's cached copy of the old one.
