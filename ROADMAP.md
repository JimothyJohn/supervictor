# ROADMAP — Scaling the Fleet

Where supervictor goes after the fleet-operations MVP, and what triggers each
step. Rethought July 2026 from the original orchestration pitch against
the codebase; re-cut 2026-10-27 when the serverless path was retired and
the TODO board was emptied. This file is the only backlog: every gated
piece of work is a bullet here with its trigger. Decisions live where they apply (`HARDWARE.md`
§11, `docs/MESH.md`, `docs/OTA.md`, `supervictor/endpoint/CLAUDE.md`);
hands-on steps that are Nick's are cards in `TODO.md` backed by tracked
`scripts/`; enterprise/monetization scope in `todo/ENTERPRISE.md`.

## Where we are (shipped)

```
ESP32 ───mTLS──► one static binary on a Linux box ──► SQLite
  (PoE Ethernet,            │  (rustls terminates mTLS; systemd unit;
   WiFi, or a               │   scripts/install-endpoint.sh)
   simulated machine)       ├── /ui        dashboard: fleet, device pages, live windows, history
                            ├── /fleet     health, summary, history as JSON
                            ├── /firmware  OTA images, pulled by the device over the same mTLS
                            └── watchdog   dark-device alerting in the journal
```

One axum binary, three PEMs, a SQLite file. Two boxes run it, both an EC2
t4g.micro from `supervictor/endpoint/template-staging.yaml` whose bootstrap
installs a GitHub release: staging (`staging.supervictor.advin.io`, the
simulated fleet) and prod (`supervictor.advin.io`, stack
`supervictor-endpoint-prod`, Let's Encrypt server certificate, v0.2.0).
One fleet CA signs every identity on both, so a board moves between them
by OTA. Routine deploys are `./scripts/ship.sh` (plan) then
`./scripts/ship.sh go`, or `make ship [GO=1]`. Nothing in the service
depends on AWS, and the same binary runs on a Pi, a VPS or in a container
(`docker-compose.yml` + Caddy). Serverless is out: API Gateway could not
carry the client certificate's identity into the binary.

Deployment is `Quickstart onboard` / `scripts/rack-flash.sh` (cert →
register → flash → verify); monitoring is `/fleet`, `Quickstart fleet` and
the dashboard; sustainment is firmware-version reporting per uplink, the
staleness watchdog, and OTA (A/B slots, sha256 + read-back verify, rollback
after three unconfirmed boots; `docs/OTA.md`). The rack — four
ESP32-POE-ISO and two ESP32-POE2, all WROOM — has been on the A/B table
since 2026-10-27 and is moving from staging to prod by OTA
(`scripts/boards-to-prod.sh`); USB is the recovery path only. Power state
is "producing": a per-device threshold plus an optional minimum on-time,
so a heater zone pulsing never reads as a machine running.

The delivery loop runs itself: PRs auto-merge into `dev` on green required
checks; weekly Dependabot bumps flow through the same gate (grouped minors
silently, majors adjudicated); cargo-audit sweeps advisories weekly. The
customer-facing site lives at www.supervictor.advin.io (GitHub Pages),
publishing from `master` on promotion.

**Operating principle, unchanged: don't orchestrate what doesn't need
orchestrating.** Each item below has an explicit trigger; before the trigger
fires, building it is toil.

## Firmware track

- **Real sensors** (trigger: the SHT41 and the clamp wiring are on the
  bench — `HARDWARE.md` §7). `sensor-current` and `sensor-sht4x` features;
  each takes its field over from the simulator. The simulator is already
  behind `sim` (off by default), so a measured-only image exists today.
- **ESP-NOW mesh** (trigger: a leaf board with an external antenna is
  bought). Contract decided and written: `docs/MESH.md` — gateway vouches,
  shared PMK, `gateway_id` on the record, beacon/join/forward/roam in that
  order, each step a bench-proven PR.
- **Board abstraction** (trigger: a non-WROOM or non-Olimex board joins the
  rack). The pattern exists — `board::active` re-exports the pin map and
  RMII bring-up chosen by `board-*` features — and the wish is an
  application-specific HAL that keeps the uplink task ignorant of which
  board it runs on. Grow it from a second board, not speculatively.
- **esp ecosystem: the 1.2 line** (trigger: `esp-radio` 1.0 ships stable).
  The edge build is on esp-hal 1.1 / esp-rtos 0.3 / esp-radio 0.18 /
  mbedtls-rs 0.2 (2026-11). esp-hal 1.2, esp-rtos 0.4, esp-storage 0.10,
  esp-bootloader-esp-idf 0.6, esp-alloc 0.11 and mbedtls-rs 0.3 are out,
  but the only esp-radio that pairs with them is a 1.0.0 beta. When it
  lands: bump the set, drop the `esp-*` minor ignores in
  `.github/dependabot.yml`, and soak on the rack before it reaches `dev` —
  CI only compile-checks firmware.
- **`ph-esp32-mac` back to crates.io** (trigger: upstream cuts a release
  after #4 and #5). Pinned today to the #5 merge commit on upstream's
  `v0.1.2-candidate` branch (MDIO fix + esp-hal 1.1 port); Dependabot cannot
  watch a git dependency, so check when touching `supervictor/edge/Cargo.toml`.
- **OTA next steps** (`docs/OTA.md`): signed manifests (O.2), generic
  images with the identity in NVS so one image serves the fleet, staged
  rollouts (O.3). Triggers: the first image built by anyone but the fleet
  owner; a fleet too large to push one device at a time.

## Endpoint track

- **Device `kind` on the record** (trigger: the next registration flow
  change). `extruder`, `press-brake`, … seeds the threshold and minimum
  on-time defaults per kind (today `scripts/sim-fleet.sh` carries that
  table in bash), names the machine on the fleet table and device page,
  and later picks per-kind charts.
- **Dashboard UX pass** (trigger: the dashboard goes customer-facing
  again; owner taste applies). The landing page dropped its demo links in
  the July review and /demo still renders placeholder device names.
- **Live re-render cost** (trigger: a wall of dashboards). A live tab
  refetches and re-renders the whole page on every uplink (≥ 150 ms
  coalescing, ~2 renders/s with the current fleet); before many concurrent
  tabs, serve a `<main>` fragment or push bucket deltas over SSE.
- **Alerting beyond logs** (trigger: >1 site or >10 devices). The
  watchdog emits structured `device went dark` WARNs; relay journald → SNS
  so a dark device pages. Shares the SNS topic the billing alarm creates
  (`scripts/billing-alarm.sh`).
- **Cert rotation** (trigger: the first device certificate nearing
  expiry). Issue → deliver over the OTA channel → device swaps → revoke
  old. OTA exists; design the device-side swap with the NVS identity work.
- **Provisioning at batch scale** (trigger: >10 devices per site).
  `rack-flash.sh` loops fine to a rack; past that, a manifest-driven batch
  mode (CSV/TOML in, certs + registrations out).
- **Owner-scoped views** (trigger: multiple customers; `todo/ENTERPRISE.md`).
  `owner_id` is already everywhere in the data model; the UI is the gap.

## Operations track

- **Cert lifecycle** (trigger: an ops window; touches live deploys).
  `deploy-staging.sh` signs with `certs/ca/ca.key` from the Mac Studio
  (single copy). Move the signing material to SSM Parameter Store / Secrets
  Manager, document issuance + rotation, keep only public certs local.
  Known: two CAs with the same subject exist — the Studio's `certs/ca`
  (2026-03-03, sha256 `CD:AC:FD:B1…`) signed everything in use; an older
  2026-03-08 CA (`E7:3F:86:F4…`) whose key is not on the Studio sat in a
  retired truststore. Retire that key when the CA moves.
- **A bigger prod box** (trigger: the fleet outgrows a t4g.micro).
  `STAGE=prod scripts/staging-stack-update.sh InstanceType=…`; the AMI pin
  keeps the instance from being replaced.
- **Long-running services** (trigger: persistent connections — MQTT
  ingest, sub-second commands). The binary already is one; a sibling
  service on the same box or a second box, not a platform change.
- **Multi-tenant isolation** (trigger: tenants needing hard isolation).
  Fargate + IAM boundaries carry it below 3+ long-running services; the
  K8s cost floor buys nothing before then.
- **Field gateways** (trigger: offline sites or local control loops). The
  mesh gateway is the first step; an RPi-class box running this same
  binary as a local buffer/dashboard is the second (`pi-edge-01` is the
  Linux reference already).

## Explicitly deferred

- **Monetization / billing** (Stripe sync, usage metering): waits on
  enterprise demand — `todo/ENTERPRISE.md` (product scope; the
  infrastructure is designed on the single-binary path when the track opens).
- **MQTT broker**: don't run one speculatively; HTTP uplinks are fine at
  current message rates.
- **Kubernetes anywhere**: no trigger on the horizon.
