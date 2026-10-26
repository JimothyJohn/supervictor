# Sourcing plan — dev kit parts, US design/build shops, budget and schedule

Companion to `HARDWARE.md` (what the kit is), `hardware/bom-devkit.csv`
(cart-ready part list) and `hardware/RFQ-prototype.md` (the package a
design/build vendor needs). Vendor facts were checked against vendor pages
on 2026-10-25; each table says how. Prices are USD unless marked EUR.

**Verification legend.** *verified* = the vendor's own page was loaded that
day and the figure read from it. *snapshot* = the figure came from a search
summary of the vendor page, age unknown. DigiKey, Mouser and Amazon block
automated readers, so every figure from them is a snapshot; the part numbers
are exact search strings and a browser settles them in minutes.

## 1. Recommendation in three lines

1. **Buy the three missing kit parts today** (SHT41, one cable, the line
   splitter; ~$30) and finish the jumper-wire bring-up in `HARDWARE.md` §8.
   Everything else waits on that data.
2. **Scope B, the 2-layer sensor hat, is the prototype to build** (decided
   2026-10-26; the RFQ is hat-only). It keeps
   the Olimex board as the certified, stocked, isolated core and costs a few
   thousand dollars end to end. Design it in KiCad in-house or at Sierra
   Circuits; build it at CircuitHub or Sierra.
3. **Scope C, the integrated node, only when you need more than ~25
   units** or a different outline. Sierra Circuits for a single contract,
   or Cardinal Peak (design) plus Screaming Circuits (build). Budget
   $12–30k and a quarter.

## 2. Component vendors

Full detail with URLs and qty-10 pricing is in `bom-devkit.csv`. The short
version:

| Part | Buy from | Part no. | Price | Stock 2026-10-25 | Status |
|------|----------|----------|-------|------------------|--------|
| ESP32-POE-ISO | Mouser (US) | 909-ESP32-POE-ISO | $29.44 | ~876 | snapshot |
| ESP32-POE-ISO | DigiKey (US) | 1188-ESP32-POE-ISO-ND | $28.19 | ~290 | snapshot |
| ESP32-POE-ISO | Olimex direct (BG) | ESP32-POE-ISO | 24.95 EUR, 22.46 at 10+, 19.96 at 50+ | in stock | verified |
| ESP32-POE-ISO-EA (u.FL antenna) | Olimex / Mouser 909-ESP32-POE-ISO-EA | | 26.95 EUR / ~$30.72 | in stock / ~250 | verified / snapshot |
| ESP32-POE-ISO-IND (-40..85 °C) | Olimex direct only in practice | | 26.95 EUR | Mouser 0, DigiKey OOS | snapshot |
| SEN0211 20 A clamp kit | DigiKey (US) | 1738-1194-ND | $19.90 | ~160 | snapshot |
| SEN0211 | DFRobot direct | SEN0211 | $19.90, $18.30 at 10+ | in stock, ships intl | verified |
| SEN0287 5 A variant | DFRobot direct | SEN0287 | $19.90 | in stock | verified |
| SHT41 STEMMA QT | Adafruit (US) | 5776 | $5.95, $5.36 at 10+ | in stock | verified |
| SHT40 STEMMA QT | Adafruit | 4885 | $5.95 | **out of stock** | verified |
| JST SH to female sockets 150 mm | Adafruit | 4397 | $0.95 | in stock | verified |
| F/F jumper pack 40 × 150 mm | Adafruit | 266 | $3.95 | in stock | verified |
| UEXT ribbon 10-pin 15 cm | Olimex | CABLE-IDC10-15cm | 2.00 EUR | in stock | verified |
| Line splitter 1×/10× | Lowe's, Zoro, Acme (per Klein buy-now page) | Klein 69409 | $19.99 | in stock | verified on kleintools.com |
| Line splitter alternate | extech.com | 480172 | $21.99 | | verified |
| Clamp meter, reference | fluke.com | Fluke 323 | $207.99 | in stock | verified |
| Clamp meter, budget | Home Depot / Lowe's | Klein CL390 | ~$100 | | snapshot |
| LS108GP (second rack only) | Micro Center / Newegg | LS108GP | $59.99 | in stock | snapshot |

Notes that change what you buy:

- **SHT41 instead of SHT40.** The SHT40 breakout is out of stock at Adafruit; the SHT41 is the same footprint, same I²C address 0x44, same driver (`sht4x` crate covers the family) and a tighter humidity spec. `HARDWARE.md` says SHT40; read it as SHT4x.
- **Olimex direct is cheapest at 10+ but ships from Bulgaria** (a week or two plus customs). For one or two boards, Mouser or DigiKey. For a 25-board order, Olimex direct and put the IDC ribbon cables in the same box.
- **Industrial-temperature POE-ISO (-IND) is thin at both US distributors.** If the deployment is outdoors or unheated, order it from Olimex direct early.
- **DFRobot direct** was verified in stock but its warehouse is not stated; expect international shipping. DigiKey carries the 20 A kit in the US.
- **No probe-only SKU.** The 5 A and 20 A kits are the same price and each includes module, probe and cable. With the register at 20 A (decided 2026-10-25) the 20 A kit is the one to standardise on; the 5 A kit is only for sub-amp loads.
- **Clamp meter.** If a true-RMS clamp meter is already on the bench, skip the Fluke. With the splitter's 10× loop any 400 A meter resolves 1 A loads.

**Cart for one kit from what is missing today:** Adafruit 5776 + 4397 (+ 266 if the jumper pack is gone) ≈ $11 plus shipping; Klein 69409 ≈ $20 from a local Lowe's. Nothing else needs buying for the bench kit.

## 3. Design and build vendors (United States)

Checked on each vendor's own site on 2026-10-25 unless marked. "Design"
means schematic plus layout as a service, not a DFM check.

### 3.1 Shortlist

| Vendor | Where | Design | Fab | Assembly | Quote path | Notes |
|--------|-------|--------|-----|----------|------------|-------|
| **Sierra Circuits** | Sunnyvale CA | yes, in-house, quoted per stage | yes, in-house | yes, in-house; Turnkey PRO 5/7/10 business days, 2–8 layers rigid | instant online for build; custom for design | The only shop found with verified design + fab + assembly under one roof. AS9100D, ISO 9001/13485, ITAR. First choice for Scope C on one contract. https://www.protoexpress.com/ |
| **Cardinal Peak** | Lafayette CO | yes: schematic, layout, bring-up | no | no, hands off to a CM of your choice | "request a technical estimate" | Espressif ESP RainMaker preferred integration partner (2024). Owned by FPT; ask about US staffing if it matters. Pair with Screaming Circuits or Sierra for the build. https://www.cardinalpeak.com/ |
| **DMC** | Chicago IL + 14 US offices | yes, plus firmware | partner | yes, own Chicago manufacturing centre for prototypes | contact sales | Industrial/IoT gateway portfolio; consultancy pricing. https://www.dmcinfo.com/ |
| **Screaming Circuits** (Milwaukee Electronics) | Canby OR | no (parent lists "engineering services", thin) | partner (ASC Sunstone) | yes; prototype specialist, qty 1–25, through-hole fine | instant online; custom quote in 3 business days | Best build-only shop for the PoE board (RJ45 magjack and flyback are through-hole). https://www.screamingcircuits.com/ |
| **CircuitHub** (with Worthington Assembly) | South Deerfield MA | no | yes (US boards for short lead) | yes, 3-day turnkey default, shared parts inventory | instant online | Ideal for Scope B spins. ISO 9001, no ITAR. https://circuithub.com/ |

### 3.2 Also usable, with a caveat each

| Vendor | Where | Caveat |
|--------|-------|--------|
| AdvancedPCB (ex Advanced Circuits + APCT + San Diego PCB Design) | Aurora CO | Layout-for-hire plus fab and 3/5/10-day assembly, no minimums; schematic ownership not stated. Published specials $66 per 4-layer bare board. https://www.advancedpcb.com/ |
| Jaycon | Palm Bay FL | Full design + firmware + assembly, KiCad native, DFM quote in under 24 h, ISO 9001; **assembly location not stated and offshore sites are mapped**. Ask before counting it as domestic. https://www.jaycon.com/pcb |
| MacroFab | Houston TX | Self-service turnkey, no minimums, ITAR; no human design service; its fast prototype tier historically excluded through-hole, so confirm the magjack and flyback don't drop you to a slow tier. https://www.macrofab.com/ |
| Simplexity | San Diego / Portland / Seattle | Design + firmware + in-house initial builds, ISO 13485; the medical-grade process is more than this needs. https://www.simplexitypd.com/ |
| Advanced Assembly, Royal Circuits (both Summit Interconnect) | Aurora CO / Hollister CA | Assembly-only / fab-only; their sites refused automated connections, confirm by phone. |
| Olimex (not domestic) | Plovdiv, Bulgaria | Owns the reference design and the parts; no custom-design page, public PCB service is suspended; direct negotiation via info@olimex.com. The wildcard if a US quote for Scope C is painful. |

**Dropped:** Tempo Automation (Chapter 7, December 2023). SparkFun no longer
advertises design or EMS services. Dojo Five is firmware-only. Voler,
Nuvation, Indesign, Ovyl and Boulder Engineering Studio are design-only
without a named build path.

### 3.3 Published rate ballparks (vendor blogs and directories, not quotes)

| Item | Range | Source |
|------|-------|--------|
| US design firm hourly | $100–180/h (senior high-speed/RF to $250) | Rapid Circuitry 2026 cost guide; circuit-board-design.com 2026 guide |
| Fixed-fee 2–4 layer design | $4,500–10,000 | Rapid Circuitry |
| "Industrial sensor node, 4-layer with Ethernet" design fee | $4,000–7,000 | Rapid Circuitry worked example |
| Fabricator NRE at prototype qty | $100–500; stencil $80–300 | Rapid Circuitry; Buildamtech 2026 |
| 4-layer proto fab + assembly, 20–40 parts, before parts | ~$20–60 per board | Buildamtech 2026 |
| Bare 4-layer 100 × 100 mm, 10 pcs | $8–18 per board at 5–10 day turn | AtlasPCB |
| Full respin | $1,500–8,000 | Rapid Circuitry |
| FCC/CE/UL for a connected product | $8,000–20,000 | Cad Crowd 2026 |

## 4. The two prototype scopes

**Scope B, sensor hat (recommended first).** A 2-layer board, roughly
28 × 40 mm, two 10-pin female headers on 1" centres that plug onto the
POE-ISO's EXT1/EXT2, carrying the SEN0211 conditioning chain (about 15
parts per the DFRobot schematic), a 3.5 mm jack, an SHT4x footprint and an
optional STEMMA QT connector. No RF, no PoE, no MCU, no compliance exposure.
Its design package is in `hardware/hat/` (schematic, netlist, BOM with
manufacturer part numbers, placement, outline, test procedure); only the
layout remains, then CircuitHub builds it in days. The Olimex board stays exactly what it is: isolated, certified
module, stocked at two US distributors.

**Scope C, integrated node.** One board derived from the Olimex Rev O KiCad
sources: ESP32-WROOM-32E, LAN8710A, Si3402-B PoE PD with the isolated
converter, CH340 USB, plus the sensor chain. Removes UEXT, microSD and LiPo.
~150 parts, 4 layers, through-hole magjack and flyback. Worth it when the
kit sells in tens per month, when the outline must change (DIN rail), or
when per-unit cost at 100+ matters (a POE-ISO is $20–28 by itself).

**Licence.** The Olimex ESP32-POE-ISO hardware is CERN OHL v1.2 (the repo's
`Licensee.pdf`). Deriving Scope C from it is permitted, including
commercial manufacture (§4.1), on the conditions that the modified design
files are released under CERN OHL v1.2 in their native format (§3.3,
§3.4(d)(e)), carry a dated change notice (§3.4(b)), keep Olimex's notices
intact (§3.1) and do not imply Olimex endorsement (§6.2). Firmware is
outside the licence (§2.3). This matches how the rest of Supervictor is
published; the RFQ tells the vendor they must return KiCad source for that
reason. The DFRobot SEN0211 schematic is published on its wiki with no
licence statement; the RFQ specifies the circuit functionally with part
substitutions allowed, which is the conservative way to use it.

## 5. Steps, with your time, cash and calendar

Estimates. "You" is bench time or reading/deciding time that only Nick can
do; firmware and documents are Claude sessions and not counted. Calendar is
wall-clock from the start of the step. Cash excludes tax and shipping.

| # | Step | Your time | Cash | Calendar | Gate to the next step |
|---|------|-----------|------|----------|----------------------|
| 0 | Order the missing kit parts (§2 cart) | 30 min | $30, +$20 splitter, +$100–210 meter if none on the bench | 2–5 days (US stock) | Parts on the bench |
| 1 | Jumper-wire bring-up, `HARDWARE.md` §8 steps 1–7 (firmware `sensor-current` / `sensor-sht4x` in parallel) | 1–2 days of bench time spread over the week, plus a 24 h soak | $0 | 1–2 weeks | Steps 3 and 6 pass; §11 decisions made (register scale, threshold) |
| 2 | ~~Pick the scope~~ (Scope B, decided 2026-10-26); fill the bracketed parts of RFQ §12 (greeting, call times, signature), attach the files | 1 h | $0 | 1 day | RFQ sent to 3 vendors |
| 3 | Quotes: two 30-minute calls, questions back and forth | 3–5 h over the period | $0 | 5–10 business days | Signed quote, PO |
| 4B | Scope B layout from the `hardware/hat/` package: KiCad here (`brew install --cask kicad`, you review) **or** Sierra's layout stage | 2–3 h review | $0 in-house; $1,500–4,000 at a shop (schematic and BOM are already done, so quote layout + DFM only) | 1–2 weeks | Gerbers released |
| 5B | Scope B build, 5 boards, CircuitHub or Sierra turnkey | 1 h ordering | $400–1,000 (setup/stencil $100–500, boards $20–40 each, parts ~$15 each) | 1–2 weeks | Boards on the bench |
| 6B | Scope B bring-up and acceptance (RFQ §8 tests 6–8) | 1 day | $0 | 1 week | Hat validated; order 25 ($40–60 each landed) |
| 4C | Scope C design, two review gates | 6–10 h (schematic review, layout review, DFM answers) | $8,000–20,000 | 4–8 weeks | Layout released, DFM clean |
| 5C | Scope C build, 10 boards, turnkey | 1–2 h | $1,500–4,000 (NRE $300–800, fab $60–100 each, assembly $40–80 each, parts $30–40 each) | 2–4 weeks | First articles pass RFQ §8 |
| 6C | Scope C bring-up, acceptance, probably one respin | 2–3 days | respin reserve $1,500–5,000 | 2–4 weeks | 10 nodes on the rack |
| 7 | Only if selling: FCC Part 15B unintentional-radiator pre-scan (the WROOM module's modular approval covers the radio), CE if EU | 1 day at the lab or none if the lab ships the report | $2,000–5,000 pre-scan; $8,000–20,000 full | 2–4 weeks | Compliance report |

**Totals.**

| Path | Your time | Cash | Calendar |
|------|-----------|------|----------|
| Kit only (steps 0–1) | ~2 days | $30–260 | 2–3 weeks |
| Kit + Scope B hat, in-house design (0–3, 4B–6B) | ~4 days | $500–1,500 | 6–9 weeks |
| Kit + Scope B at a design shop | ~4 days | $2,000–5,500 | 7–10 weeks |
| Kit + Scope C integrated node (0–3, 4C–6C) | ~6 days | $12,000–30,000 | 12–20 weeks |
| Add compliance for sale | +1 day | +$2,000–20,000 | +2–4 weeks |

The dominant cost in every path is Scope C NRE, and the dominant delay is
quote turnaround plus the two design review gates. Scope B has neither.

## 6. What is not verified and how to close it

- DigiKey and Mouser stock and price for the POE-ISO, SEN0211 and Adafruit
  5776: open the eight part numbers in a browser (5 minutes).
- Whether the SEN0211 module reaches 2.45 V output on a 3.3 V supply (its
  LDO is 3.0 V with ~0.3 V headroom): bring-up step 3 answers it.
- Sierra Circuits' design-stage pricing and whether they accept a
  CERN-OHL-derived KiCad start: RFQ question 3.
- Jaycon's assembly location, Advanced Assembly's and Royal Circuits' sites
  (refused automated connections): one phone call each, only if the
  shortlist falls through.
- Rate ballparks in §3.3 are blog numbers from 2026, not quotes; the RFQ
  asks for NRE and per-unit separately so the quotes replace them.
