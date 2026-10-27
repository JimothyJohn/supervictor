# Themes

The fleet dashboard ships six looks instead of a light/dark switch. Each was
drawn for a kind of business that runs machines on a current clamp; all six
read the same data the same way, so a device keeps its colour and `on` stays
green whichever world the viewer picks. The palette button in the header
(three swatches) opens the picker: hover a card to try it on, click to keep
it, and the panel below it makes the look yours.

| id | name | drawn for | scheme |
|---|---|---|---|
| `foundry` | Foundry | manufacturing, machine shops, control rooms | dark (the default) |
| `clinical` | Clinical | hospitals, labs, pharma cold rooms | light (the OS-light default) |
| `boardroom` | Boardroom | facilities, commercial real estate, energy managers | light |
| `neon` | Neon | breweries, venues, hospitality | dark |
| `phosphor` | Phosphor | data centres, IT ops | dark |
| `greenhouse` | Greenhouse | farms, vertical growers, cold chain | light |

"Auto" removes the choice: Foundry when the OS is dark, Clinical when light.
A `light` or `dark` choice stored by the old toggle maps onto those two.

## Make it yours

Every tweak composes with every theme, lives in the viewer's browser
(`localStorage`), and never reaches the server:

- **Accent** — any colour; the theme's own is one click away.
- **Corners** — 0 to 24 px; the large radius follows at 1.5× + 2.
- **Density** — compact / cozy / relaxed (scales the root font size, so
  every rem-based padding follows).
- **Type** — the theme's face, or monospace / sans / serif / rounded.
- **Background texture** and **motion** on or off.

"Share or save this look" gives the look as JSON (paste it back into the
same box on another browser) or as a link:

```
https://<endpoint>/ui#theme=neon&accent=ff2d95&radius=14&density=relaxed&font=sans&texture=0&motion=0
```

Opening such a link applies and stores the look once, then clears the
fragment. Only `theme` is required; unknown ids fall back to auto, bad
values are dropped.

## Semantics that never move

| token | meaning |
|---|---|
| `--green` | powered on, fresh |
| `--red` | powered off, dark |
| `--warn` | stale, polling, banners |
| `--accent` | the brand: links, the current tab, thresholds |
| `--s0`…`--s7` | device series slots, assigned by registration order |

The series slots are the two validated steps of the reference palette
(dark steps on dark themes, light steps on light ones) and are not themed:
validated contrast on every surface, and a device's colour survives a theme
switch. Three light-surface slots sit under 3:1 and are relieved by the
legend keys and the table view, as before.

## Adding a theme

1. A `:root[data-theme="<id>"]` block in `supervictor/endpoint/src/ui/themes.css`
   declaring every token the others declare (`color-scheme`, `--bg`,
   `--surface`, `--border`, `--text`, `--muted`, `--accent`, `--warn`,
   `--green`, `--blue`, `--red`, `--font`, `--font-display`, `--radius`,
   `--radius-lg`, `--label-case`, `--label-tracking`, `--card-shadow`,
   `--card-top`, `--card-left`, `--header-rule`, `--glow`, `--texture`,
   `--texture-size`), and the id added to the dark or light series rule at
   the top of the file.
2. A row in `THEMES` in `supervictor/endpoint/src/ui/themes.rs` whose
   preview hexes equal the block's `--bg`, `--surface`, `--accent`, `--text`.
3. At most a few signature rules at the bottom of `themes.css` for what
   tokens cannot say (Phosphor's cursor, Neon's glow).

`cargo test -p supervictor-endpoint --test test_ui` checks all three stay in
step. Validate any new series colours with the dataviz palette validator
before changing the slots; the chrome can be anything, the data colours
cannot.
