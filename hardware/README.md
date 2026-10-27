# hardware/ — the Supervictor telemetry node, from dev kit to board

Open `index.html` for the visual walkthrough (2D layout, 3D stack, what is
needed from Nick). The text files are the engineering package.

| Path | What |
|------|------|
| `index.html` | Front page: kit at a glance, 2D top view, rotatable 3D view, the needed-from-you checklist, budget and schedule summary |
| `../HARDWARE.md` | Dev kit spec: board choice, pin budget, wiring, ADC analysis, bring-up checklist, open decisions |
| `bom-devkit.csv` | Cart-ready parts list for the jumper-wire kit, with vendor part numbers and stock as of 2026-10-25 |
| `SOURCING.md` | Component vendors, US design/build shops, the two prototype scopes, licence, and per-step time/cash/calendar estimates |
| `RFQ-prototype.md` | The package to send a design/build vendor: scope, fixed pin map, electrical spec, deliverables, acceptance tests, cover email |
| `hat/` | **SV-HAT-A** sensor hat design package (Scope B): `DESIGN.md`, `schematic.svg`, `netlist.csv`, `bom.csv`, `placement.csv`, `outline.svg` |

## Status

- Dev kit (POE-ISO + SEN0211 on jumpers): specified, parts on hand except
  the SHT41 and one cable. Bring-up not yet run.
- SV-HAT-A: design-complete, layout-pending. No KiCad on this machine; the
  Gerbers come from a vendor or a KiCad session (`brew install --cask
  kicad`, then import `netlist.csv` and `placement.csv`).
- Integrated node (Scope C): RFQ only. Its design files are the Olimex
  ESP32-POE-ISO Rev O KiCad project (CERN OHL v1.2) plus the hat circuit.

## Note on the docs site

`docs/` is the published site; `hardware/index.html` is a standalone page
that loads three.js from cdnjs and the two fonts from Google Fonts, nothing
else. Open it from the checkout or copy it under `docs/` if it should be
public.
