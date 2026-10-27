# Mesh — ESP-NOW leaves behind a PoE gateway

Decided 2026-10-27 (board answer `mesh-identity-model`): **the gateway
vouches for its ESP-NOW children, link encryption is a shared PMK baked at
flash time.** Bench-grade, not per-leaf certificates. This page is the
contract the protocol gets built against; the coexistence spike it stands
on is in the tree (`supervictor/edge/src/network/espnow.rs`, feature
`espnow`).

## What exists

The spike (2026-10-25, PR 103) proved ESP-NOW beacons on the WiFi radio
beside the wired EMAC uplink on the ESP32-POE-ISO: `--features
ethernet,espnow` on `poe-iso-rack-02` and `-03`, 29/29 beacons heard, no
gaps, both boards uplinking every 5 s throughout, every rack row fresh.
Cost: the radio blob is ~34 KB of DRAM; every classic-ESP32 build now runs
an 80 KB primary heap plus the 96 KB `dram2_seg` second heap (3 h soak).
The radio comes up in station mode without joining a network, so a wired
board needs no SSID.

## Roles

- **Gateway** — a PoE board (`ethernet` + `espnow`). Holds the device
  certificate, owns the mTLS uplink, and relays its leaves' readings. It
  is the only thing the endpoint ever talks to.
- **Leaf** ("moth") — a battery or line-powered board with **no Ethernet
  and an external 2.4 GHz antenna**, meant to live in the open (outside
  the machine's enclosure, where an internal antenna would be screened)
  and to **hunt for signal**: it listens for gateway beacons, picks the
  strongest, and re-picks when that one goes quiet. A leaf never needs a
  network credential, a certificate or a hostname; it needs the PMK and a
  name.

## Identity and trust

- The leaf's identity on the wire is its device id, carried inside the
  gateway's uplink. The endpoint's device record gains `gateway_id` (the
  gateway that vouched) and a leaf is `active` only while it is bound to
  a gateway that is itself bound to its certificate. Revoking the gateway
  silences its leaves; revoking a leaf is a record change like any other.
- Authorisation stays where it is: the endpoint checks the gateway's mTLS
  subject exactly as today (`handlers::authorize_uplink`). A leaf reading
  arriving under a gateway the leaf is not bound to is rejected and logged.
- **Link encryption**: ESP-NOW's primary master key (PMK) and a per-fleet
  local master key, both baked at flash time like `SSID` (compile-time
  env, never in NVS, never on the wire). Replacing the PMK is a reflash of
  every leaf and gateway — acceptable at bench scale, and the reason this
  is bench-grade: a captured leaf gives up the fleet's PMK. Per-leaf
  certificates return when leaves leave the bench.
- Replay: every leaf frame carries the leaf's `seq`; the gateway drops a
  repeat and the endpoint already rejects a stale `seq` per device.

## Wire shape (to build)

1. Gateway beacon every `BEACON_S` (the spike's cadence): gateway id,
   fleet tag, uptime. Encrypted with the PMK; a leaf with the wrong fleet
   tag ignores it.
2. Leaf join: the leaf answers the strongest beacon with its id and
   firmware version; the gateway adds it to a small peer table (ESP-NOW
   caps encrypted peers at a handful — the table size is the leaf budget
   per gateway).
3. Leaf reading every uplink interval: `current` plus whatever the leaf
   measures, same quantised `levels` burst as a wired board
   (`supervictor_common::levels`), so the endpoint's ingest path does not
   change. The gateway forwards each leaf reading as its own uplink body
   with `id` = the leaf's id — one HTTP POST per reading, no new route.
4. Roaming: a leaf that misses `N` beacons from its gateway re-scans and
   re-joins; the endpoint sees `gateway_id` change on the next uplink.
5. OTA for leaves: the gateway pulls the leaf image over its uplink and
   streams it over ESP-NOW in 250-byte frames into the leaf's inactive
   slot (same A/B machinery, `docs/OTA.md`). Last.

## Hardware notes

- The gateway is any rack PoE board; the external-antenna leaf is the
  Olimex boards' `-EA` / U.FL variants (ESP32-POE-ISO-EA for a PoE leaf,
  or a plain WROOM-32UE devkit on a battery). Nothing in the kit spec
  (`HARDWARE.md`) changes for the gateway; the leaf gets its own BOM row
  when it is bought.
- ADC2 is unusable while the radio is on (`HARDWARE.md` §5.1): the clamp
  stays on ADC1 for gateways and leaves alike.
- The ST25DV NFC tag (`HARDWARE.md` §12) is how a leaf could be named
  and bound in the field (tap: leaf id + gateway id) — it waited on this
  decision and can be planned now.

## Order of work

Beacon + join on the spike module (two rack boards), the `gateway_id`
record change and the forwarding path on the endpoint, then a real leaf
with an external antenna, then roaming. Each step is its own PR with a
bench proof in the description; nothing here needs a USB flash beyond the
first leaf.
