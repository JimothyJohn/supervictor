# SV-HAT-A — sensor hat for the Olimex ESP32-POE-ISO

Design package for the Scope B board in `hardware/RFQ-prototype.md`. This
is design-complete and layout-pending: schematic, netlist, BOM, placement
intent, mechanical envelope and the test procedure are here; the layout
(Gerbers, drill, pick-and-place from a real CAD tool) is the one step a
vendor, or a KiCad session on this machine, still produces. No KiCad is
installed here (`kicad-cli` absent on 2026-10-25), which is why the files
are text and SVG rather than `.kicad_*`.

## Files

| File | What | Who consumes it |
|------|------|-----------------|
| `schematic.svg` | Readable schematic, one sheet | you, the design shop |
| `netlist.csv` | Every pin of every part and its net; the machine-readable truth the schematic is drawn from | the layout engineer |
| `bom.csv` | Parts with manufacturer part numbers, package, qty, approved alternates | turnkey assembly |
| `placement.csv` | Component positions and orientations in mm from the board's lower-left corner; the placement intent the layout starts from | the layout engineer |
| `outline.svg` | Board outline, header positions, keep-outs and slot, to scale | layout, mechanical check |
| this file | Design rules, layout notes, test procedure | everyone |

## What it does

A 2-layer, 28 × 40 mm board (with a 16 × 13 mm notch over the UEXT) that
plugs onto the POE-ISO's EXT1/EXT2 headers and carries:

- the DFRobot SEN0211 conditioning chain (3.0 V LDO, 2× non-inverting
  amplifier, Schottky peak detector with a 10 s hold) from a 3.5 mm jack to
  GPIO32 through a 1 kΩ / 100 nF low-pass at the ESP32 pin;
- an SHT41 on the UEXT I²C bus (GPIO13 SDA, GPIO16 SCL), using the 2.2 kΩ
  pull-ups already on the POE-ISO;
- a STEMMA QT / Qwiic connector on the same bus for an IMU or a second
  sensor later;
- three test points: ADC_OUT, GND, AC_IN.

Every other header pin is no-connect. Nothing on the hat drives a boot
strap pin, UART0, or the Ethernet pins.

## Electrical

| Item | Value |
|------|-------|
| Supply | +3.3 V from EXT1 pin 2, < 10 mA total (LDO quiescent 1.6 µA, LMV321 ~0.1 mA, SHT41 < 1 mA peak) |
| Analog rail | 3.0 V (MCP1700-3002), 4.7 µF + 100 nF each side |
| Input | 0–1 V AC RMS at the jack tip, 47 kΩ termination, Schottky clamp to GND for the negative half-cycle |
| Gain | 2 (47 kΩ / 47 kΩ), non-inverting |
| Detector | 1N5819HW into 10 µF, 1 MΩ bleed: τ = 10 s |
| Output | 0–~2.8 V DC at ADC_RAW; through 1 kΩ to the pin with 100 nF |
| ESP32 ADC | ADC1_CH4, 11 dB attenuation, linear 150–2450 mV (≈1.06–17.3 A) |
| I²C | 3.3 V, 100 kHz, SHT41 at 0x44, pull-ups on the host board |

## Layout rules (hand these to the layout engineer)

- 2 layers, 1.6 mm FR-4, 1 oz copper, HASL or ENIG, green or any.
- Bottom layer is a ground pour; top layer routes. Minimum 0.2 mm track /
  0.2 mm space, 0.3 mm drill minimum. Nothing here needs better.
- Header pin 1 of J1 and J2 is at the +Y end (toward the ESP32 module when
  the hat is mounted). Header centres 25.40 mm apart in X, 10 pins on
  2.54 mm along Y, centred at Y = 20.00. Silkscreen a "1" at each pin 1
  and an arrow "ESP32 →" at the +Y edge.
- Keep the analog chain (J3 → R3/D2 → IC1 → D1 → C7/R4 → R5/C8) in one
  short run along the board centre; the ADC node must not pass under the
  LDO or near the headers' unused pins.
- **UEXT notch.** The POE-ISO's boxed UEXT header sits between the EXT
  headers under the hat's +Y end and is taller than a standard 8.5 mm
  socket. The hat has a 16 × 13 mm notch, X 6–22, Y 27–40, open at the
  +Y edge (the UEXT box is 15.5 × 9 mm, lying across the board width), so it clears the UEXT with any socket height and leaves the UEXT
  usable with the hat fitted. Do not place anything in the notch.
- SHT41 in the +X/+Y corner, with the notch as its −X air gap, a 1 mm
  routed slot under it (Y 29.5–30.5, X 21–26) and on its +X side
  (X 25.6–26.6, Y 29.5–36), no copper pour under it; the LDO sits in the
  3.4–5.0 mm strip on the other side of the notch and the op-amp is lower on
  the board.
- The 3.5 mm jack body hangs over the POE-ISO when mounted; its opening
  faces −Y (the RJ45 end). Nothing below it on the host board is taller
  than the header stack (8.5 mm socket + 1.6 mm PCB ≈ 10 mm clearance).
- No mounting holes; the two 10-pin sockets carry the board.
- Silkscreen: board name `SV-HAT-A`, revision, the three test point
  labels, `TIP=SIGNAL` next to the jack, and the CERN-OHL-free notice
  "own design, MIT" (this board is not derived from the Olimex files;
  only its header geometry is).

## Assembly notes

- Through-hole: J1, J2, J3 (three parts). Everything else is 0603/0805,
  SOT-23, SOD-123, DFN-4 (U2, 1.5 × 1.5 mm, needs a stencil and reflow;
  the one part not hand-solderable).
- Diode orientation is the classic assembly error: D1 cathode toward C7,
  D2 cathode toward the jack. Both are called out in `placement.csv`.
- Lead-free, no conformal coat, no wash required (SHT41 dislikes wash
  residue; if the shop washes, ask for the Sensirion protective cover or a
  no-clean process).

## Test procedure (per board)

1. Visual: diode bands, U2 orientation dot, header pin 1 marks.
2. Unmounted, bench supply 3.3 V on J1-2/J1-3 with a 100 mA limit: draw
   < 5 mA; +3V0 at C3 = 3.00 ± 0.06 V.
3. Function generator into J3, 60 Hz sine: 0.300 V RMS → TP1 = 0.80–0.90 V
   DC after 30 s; 1.000 V RMS → TP1 ≥ 2.7 V; 0 V → TP1 < 0.05 V after 60 s.
4. Mount on a POE-ISO running the `sensor-current,sensor-sht4x` build:
   `current` tracks step 3 at 5.5–6.5 A for the 0.300 V input; `temp_c`
   and `humidity_pct` present and plausible.
5. Plug an Adafruit STEMMA QT sensor into J4: it is seen on the bus (any
   I²C scan or the firmware's log).

## Licence

This hat is an original design (the conditioning chain follows the
published DFRobot SEN0211 schematic functionally; the header geometry is
the Olimex EXT1/EXT2 pitch). Released under the repository's MIT licence.

## Verified pinouts

- LMV321 DBV (SOT-23-5), TI SLOS263Y table 5-2: 1 IN+, 2 GND, 3 IN−, 4 OUT,
  5 VCC+. The DCK (SC-70) has the same order. Input common-mode range is
  0 to VCC−1.0 V, so with the 3.0 V rail the 1.41 V peak input is inside it.
- MCP1700 SOT-23-3: 1 GND, 2 VIN, 3 VOUT (Microchip MCP1700 datasheet).
- SHT41 DFN-4: 1 SDA, 2 VDD, 3 VSS, 4 SCL (Sensirion SHT4x datasheet).
- STEMMA QT / Qwiic JST SH order: 1 GND, 2 V+, 3 SDA, 4 SCL.
- SJ1-3523N contact numbering: not verified here; check the Same Sky
  drawing before the footprint is finalised (netlist assumes 1 = sleeve,
  2 = tip, 3 = ring).
