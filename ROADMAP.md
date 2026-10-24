# ROADMAP — Scaling the Fleet

Where supervictor goes after the fleet-operations MVP, and what triggers each
step. Rethought July 2026 from the original orchestration pitch
(`todo/orchestration.md`, now retired) against the current codebase.
Tactical backlog lives in `TODO.md`; enterprise/monetization scope in
`todo/ENTERPRISE.md`.

## Where we are (Phases 0–1: shipped)

```
ESP32-C3 ──mTLS──► API GW ──► Lambda ──► DynamoDB        (serverless path)
ESP32-C3 ──mTLS──► Caddy ──► axum on t4g.micro ──► SQLite (self-hosted path)
                               │
                               ├── /ui     dashboard (SSR + SSE, admin mTLS)
                               ├── /fleet  health API (staleness, fw versions)
                               └── watchdog (dark-device alerts in logs)
```

The original pitch's "portable path" is real: one axum binary runs in Lambda
(Web Adapter), on the staging box, or in any container. `./Quickstart` is the
single entry point (the `qs` CLI is retired): deployment is
`Quickstart onboard` (cert → register → flash); monitoring is `/fleet`,
`Quickstart fleet`, and the dashboard; sustainment is firmware-version
reporting per uplink plus the staleness watchdog. Release artifacts ship the
endpoint for Linux aarch64 (the t4g staging host). API Gateway is explicitly
throttled and the Lambda concurrency-capped.

The delivery loop runs itself: PRs auto-merge into `dev` on green required
checks; weekly Dependabot bumps flow through the same gate (grouped minors
silently, majors adjudicated — three esp-ecosystem crates are ignored until
the Phase-2 esp migration); cargo-audit sweeps advisories weekly. The
customer-facing site lives at www.supervictor.advin.io (GitHub Pages,
HTTPS-enforced), publishing from `master` on promotion.

**Operating principle, unchanged: don't orchestrate what doesn't need
orchestrating.** Each phase below has an explicit trigger; before the trigger
fires, building it is toil.

## Phase 2 — Sustainment automation (trigger: >1 site or >10 devices)

The gap that appears with real fleet scale is *acting* on what monitoring
shows:

- **OTA updates.** `/fleet` now shows which devices run old firmware; OTA is
  how you fix it without a truck roll. esp-hal ecosystem provides
  `esp-hal-ota`-style A/B partitions; the endpoint grows a firmware-artifact
  route and a per-device desired-version field. This is also the
  prerequisite for cert rotation (todo/ENTERPRISE.md).
- **Cert rotation.** Issue new device cert → deliver via OTA channel →
  device swaps → revoke old. Needs OTA first; design both together.
- **Alerting beyond logs.** The watchdog emits structured `device went dark`
  WARNs; wire a CloudWatch metric filter + alarm (serverless path) or a
  journald → SNS relay (self-hosted) so a dark device pages instead of
  waiting to be read. Pairs with the TODO P0 billing alarm — both need the
  same SNS topic + alert-endpoint decision.
- **Provisioning at batch scale.** `Quickstart onboard` loops fine to ~10; past
  that, a manifest-driven batch mode (CSV/TOML in, certs + registrations
  out) keeps a site install to one command.

## Phase 3 — Real-time & long-running services (trigger: persistent connections)

Lambda can't hold a socket open. The first workload that needs one — MQTT
ingest, WebSocket push to dashboards behind API GW, sub-second command
delivery to devices — moves the *service*, not the architecture:

- The endpoint binary already runs as a plain server; deploy it (or a
  sibling service) on **ECS Fargate** with the existing container pattern.
  The staging t4g box is the dress rehearsal for exactly this.
- The dashboard's SSE reload-fallback on Lambda is the built-in trigger
  signal: when live push matters on the AWS path, that's the moment.
- Step Functions enters here too if the enterprise workflows land
  (provisioning sagas, billing sync) — multi-step, retryable, stateful
  operations that shouldn't be Lambda-calling-Lambda.

## Phase 4 — Multi-tenant isolation (trigger: multiple customers)

When tenants need hard isolation (data, quotas, blast radius):

- Owner-scoped views exist in the data model (`owner_id` everywhere);
  Phase 4 of the dashboard plan adds the UI.
- Evaluate **EKS** only at 3+ long-running services with per-tenant
  isolation demands; below that, Fargate + IAM boundaries carry it.
  The K8s cost floor (~$75/mo control plane + one engineer's attention)
  buys nothing before then.

## Phase 5 — Field gateways (trigger: offline sites or local latency)

10+ devices in one physical location, flaky uplink, or sub-second local
control loops → a gateway (RPi-class) between devices and cloud:

- Local buffer (SQLite — same store code), aggregation, OTA relay,
  local dashboard (same axum binary — this is why everything stays
  portable).
- K3s or Balena for gateway fleet management; decide when a real site
  exists, not before.

## Explicitly deferred

- **Monetization / billing** (Stripe sync, usage metering): whole track
  waits on enterprise demand — see `todo/ENTERPRISE.md`.
- **MQTT broker**: don't run one speculatively; HTTP uplinks are fine at
  current message rates.
- **Kubernetes anywhere**: no trigger on the horizon.
