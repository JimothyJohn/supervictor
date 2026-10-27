# ROADMAP — Scaling the Fleet

Where supervictor goes after the fleet-operations MVP, and what triggers each
step. Rethought July 2026 from the original orchestration pitch
(`todo/orchestration.md`, now retired) against the codebase; re-cut
2026-10-27 when the serverless path was retired and the TODO board was
emptied. This file is the only backlog: every gated piece of work is a
bullet here with its trigger. Decisions live where they apply (`HARDWARE.md`
§11, `docs/MESH.md`, `docs/OTA.md`, `supervictor/endpoint/CLAUDE.md`);
hands-on steps that are Nick's live in `scratch/README.md` (untracked);
enterprise/monetization scope in `todo/ENTERPRISE.md`.

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

One axum binary, three PEMs, a SQLite file. The reference box is one EC2
t4g.micro (`supervictor/endpoint/template-staging.yaml`,
`staging.supervictor.advin.io`) whose bootstrap installs a GitHub release;
nothing in the service depends on AWS, and the same binary runs on a Pi, a
VPS or in a container (`docker-compose.yml` + Caddy). The API Gateway +
Lambda path is gone (2026-10-27): it could not carry the client
certificate's identity into the binary, and "why are we still using Lambda
when we can run this on our own naked infra anywhere" was the answer.

Deployment is `Quickstart onboard` / `scripts/rack-flash.sh` (cert →
register → flash → verify); monitoring is `/fleet`, `Quickstart fleet` and
the dashboard; sustainment is firmware-version reporting per uplink, the
staleness watchdog, and OTA (A/B slots, sha256 + read-back verify, rollback
after three unconfirmed boots; `docs/OTA.md`). The rack — four
ESP32-POE-ISO and two ESP32-POE2, all WROOM — has been on the A/B table
since 2026-10-27; USB is the recovery path only. Power state is "producing":
a per-device threshold plus an optional minimum on-time, so a heater zone
pulsing never reads as a machine running.

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
- **esp ecosystem migration** (trigger: an on-device soak window). Bump
  `esp-rtos` 0.2 → 0.3 and `esp-radio` 0.17 → 0.18, then unpin
  `esp-hal =1.0.0` and drop the three Dependabot ignores (esp-hal,
  embassy-executor, esp-alloc). CI only compile-checks firmware; a
  scheduler/radio migration needs hours on the rack before it reaches
  `dev`, so it waits for a day when the fleet can be dark.
- **`ph-esp32-mac` back to crates.io** (trigger: upstream cuts a release
  after #4). Pinned today to the upstream merge commit; Dependabot cannot
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
  (`scratch/04-billing-alarm.sh`).
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
- **A production box** (decided 2026-10-27: Path A, "make prod a
  staging-shaped box"; in progress). `PROD_HOST` (`supervictor.advin.io`)
  is wired through every script as a second box on the single-binary path,
  and `STAGE=prod` points the box scripts (`deploy-staging.sh`,
  `install-staging.sh`, `staging-letsencrypt.sh`, `staging-stack-update.sh`,
  `deploy-staging-build.sh`, `teardown-staging.sh`) at it: stack
  `supervictor-endpoint-prod`, PEMs under `s3://supervictor/prod/`, AMI pin
  `/supervictor/prod/ami-id`. Order: tear down the Lambda stacks (frees the
  name), cut v0.2.0, `STAGE=prod TAG=v0.2.0 deploy-staging.sh`,
  `STAGE=prod staging-letsencrypt.sh`, then move boards with
  `VIA=staging ota-push.sh <device> prod` after registering them on prod.
  One fleet CA across both boxes, so a board moves by OTA; a bigger
  instance class when the fleet grows (`staging-stack-update.sh InstanceType=`).
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
  enterprise demand — `todo/ENTERPRISE.md` (its infrastructure sections
  predate the serverless retirement and are redone when the track opens).
- **MQTT broker**: don't run one speculatively; HTTP uplinks are fine at
  current message rates.
- **Kubernetes anywhere**: no trigger on the horizon.
