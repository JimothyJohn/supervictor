# TODO — Engineering Roadmap

Prioritized backlog of concrete improvements. The scaling plan lives in
`ROADMAP.md`; larger strategic tracks in `todo/` (ENTERPRISE.md,
open-source-framework.md). Completed items are deleted in the PR that lands
them — git history is the changelog. Every remaining item is gated; the
**unblocks** note says what has to happen first.

## P0 — Security & cost guardrails

- [ ] Billing alarm: CloudWatch `EstimatedCharges` alarm + SNS topic. Shares
      the topic/endpoint with ROADMAP Phase 2's dark-device alerting.
      **Unblocks:** an alert email/endpoint decision + account-level billing
      alerts enabled.
- [ ] Cert lifecycle: `deploy-staging.sh` signs with `certs/ca/ca.key` from
      the local working tree (gitignored, single copy on one laptop). Move
      signing material to SSM Parameter Store / Secrets Manager; document
      issuance + rotation; keep only public certs local. **Two CAs exist
      with the same subject** (found 2026-10-24 while repairing the prod
      truststore): the Mac Studio's `certs/ca` (issued 2026-03-03, sha256
      `CD:AC:FD:B1…`) signed every device/admin/server cert in use, and prod
      now trusts it (truststore.pem version `1fmZKZzmgktCHdWvGdb4PPTv6oJUWG_S`).
      An older truststore held a 2026-03-08 CA (`E7:3F:86:F4…`) whose key is
      not on the Studio; API Gateway refuses a truststore with both (same
      subject). Find or retire that key when the CA moves to SSM.
      **Unblocks:** an ops window — touches live staging deploys.

## P1 — Hygiene

- [ ] `dev` branch protection contexts (owner-run): required checks are
      `Lint + test (host)` and `Embedded build (riscv32, ESP32-C3)` (checked
      2026-10-25; the esp32-poe-iso merge kept the job name). The
      `DynamoDB store (DynamoDB Local)` job runs on every PR but is not
      required, so a dynamo-only failure cannot block auto-merge. Add it:
      `gh api -X PUT repos/JimothyJohn/supervictor/branches/dev/protection`
      per ~/.claude/templates/github/README.md.
      **Unblocks:** owner (admin token).
- [ ] Simulated fleet: the ten `sim-*` containers on the Studio were started
      from the worktree `../supervictor-sim-profiles` (its untracked `certs`
      symlink feeds the compose bind mounts). Move the run to the main
      checkout once it is on `dev` ≥ 6912de9: `./scripts/sim-fleet.sh down`
      in the worktree, `./scripts/sim-fleet.sh up staging` in the checkout,
      then `git worktree remove ../supervictor-sim-profiles`.
      **Unblocks:** the main checkout leaving `feat/power-state`.
- [ ] Flip `REQUIRE_REGISTRATION=true` in template.yaml (prod) once every
      live device is registered *with* its `subject_dn` (strict mode
      rejects unbound registrations — owner call 2026-10-23). `GET /fleet`
      shows who still lacks one; `Quickstart onboard` re-registers.
      **Unblocks:** fleet re-registration pass.

## P1b — Single-binary deployment (owner direction, 2026-10-23)

Goal: `supervictor-endpoint` runs on any generic Linux box with no Docker,
no Caddy, no on-box source build — one static binary, three PEMs, a
SQLite file, a systemd unit. Single instance; scaling TBD. Today's
staging path (EC2 → dnf docker → compose → `cargo build` on a t4g.micro
with swap → Caddy for mTLS) is the thing this replaces. Approach chosen:
in-binary mTLS via rustls (decision 2026-10-23). Landed: `tls` feature +
`TlsListener`, musl release matrix, `scripts/install-endpoint.sh` +
hardened unit (`supervictor/endpoint/deploy/`). **Running since
2026-10-24:** `v0.1.0` (aarch64 musl) on the staging box
`staging.supervictor.advin.io` (i-0672688a9ae6a2614, t4g.micro, SQLite,
`REQUIRE_REGISTRATION=false`) — installed by hand over SSM; device ping,
admin `/fleet/summary`, and no-cert refusal all verified. The old
compose/Caddy stack on that box is stopped and disabled, docker disabled.

- [ ] Retire the compose/Caddy staging path: `template-staging.yaml`
      UserData becomes "run the install script"; `docker-compose.staging.yml`,
      `Caddyfile.staging`, and `scripts/deploy-staging.sh`'s bundle upload go.
      The stack `supervictor-endpoint-staging` is already drifted: its EIP was
      released at some point, so a new one (`100.56.171.238`, tagged
      `Name=supervictor-staging`) was allocated out-of-band and the Route53
      record repointed. Fold that EIP into the template when it is rewritten.
      **Unblocks:** an ops window.
- [ ] The box runs a hand-installed `dev` build (6912de9, deployed
      2026-10-25 with the worktree copy of `scratch/deploy-staging-build.sh`:
      musl build in an arm64 container → S3 → SSM install). `install-staging.sh`
      without a `TAG` still rolls it back to v0.1.0. Cut v0.2.0 (P4) and
      install from the release so staging is reproducible.
      **Unblocks:** the v0.2.0 tag (owner push).

## P2 — Firmware (edge)

- [ ] Decide the `portal` feature's fate: it `include_bytes!`s
      `portal/dist/supervictor_portal_bg.wasm.gz`, which is not in the tree —
      the feature cannot build. Either add a portal build step (wasm
      toolchain) or drop the feature until the portal returns.
      **Unblocks:** owner decision (build it vs. drop it).
- [ ] esp ecosystem migration: bump `esp-rtos` 0.2 → 0.3 and `esp-radio`
      0.17 → 0.18, then unpin `esp-hal =1.0.0` (pinned because 1.1.x removed
      unstable APIs the older crates call). Remove the three Dependabot
      ignores that lift with it (esp-hal, embassy-executor, esp-alloc).
      **Unblocks:** flashable hardware on hand — CI only compile-checks the
      firmware; a scheduler/radio migration needs an on-device soak.
- [ ] Drop the heapless-0.8 bridge in `desktop_main.rs` once serde-json-core
      releases against heapless 0.9 (their main branch already moved; we're
      unified on 0.9.3 everywhere else).
      **Unblocks:** a serde-json-core release > 0.6.0.
- [ ] Finish the wired-Ethernet soak on the ESP32-POE-ISO: cable pull/replug
      recovery, plus a multi-hour uplink soak. The rest of the checklist was
      hardware-verified 2026-07-15 — PHY ID `0x0007c0f1` on console, link at
      100FD, DHCP lease, repeated mTLS uplink roundtrips (HTTP 200) with
      clean keepalive-expiry reconnects. The POE2 has its own wired path
      since PR 83 (`board-poe2`: RMII clock out of GPIO0); it still needs
      its first cable test (switch day below).
      **Unblocks:** a hand on the cable (pull/replug is physical) and an
      hours-long soak window.
- [ ] Simulator behind a cargo feature once the first real sensor ships
      (`sim`, off by default) so a production board can never report an
      invented reading; `SIM_PROFILE` builds keep it on. Today
      `models/sim.rs` + `models/profile.rs` are compiled in unconditionally
      and fill temp_c / humidity_pct / rssi_dbm (heap_free and uptime_s are
      real).
      **Unblocks:** the first real sensor (owner answer on the open
      question below).
- [ ] Move `ph-esp32-mac` from the pinned git fork back to crates.io. The
      released 0.1.1 can't detect any PHY (MDIO reads broken — upstream
      photon-circus/ph-esp32-mac#4); we pin the PR's hardware-verified fix
      rev from `cfbraun/ph-esp32-mac`. If the fork disappears before #4
      merges, builds break — re-pin or vendor.
      **Unblocks:** upstream merging #4 and cutting a release.
- [ ] OTA update path (prerequisite for cert rotation on deployed devices —
      see todo/ENTERPRISE.md and ROADMAP.md Phase 2).
      **Unblocks:** hardware + the esp migration above (A/B partition
      support lands with the newer esp-hal line).

## P3 — Endpoint

- [ ] Dashboard UX rework: the live dashboard needs design attention before
      it's customer-visible again — the landing page dropped its demo links
      in the July review, and /demo still renders with placeholder device
      names. Relink from the landing page when it's presentable.
      **Unblocks:** a design pass (owner taste applies).
- [ ] Fleet dashboard Phase 4 (owner-scoped views, billing summaries).
      **Unblocks:** the enterprise data model (todo/ENTERPRISE.md).
- [ ] Dashboard on Lambda: SSE degrades to reload-fallback behind API GW
      buffering (by design) — the live 1m/5m windows fall back the same way
      (reload after three stream errors). If live push matters there,
      that's the ECS-migration trigger per ROADMAP.md Phase 3.
      **Unblocks:** a real need for live push on the serverless path.
- [ ] "Producing" vs "powered" for heated machines: one threshold per device
      can't separate a heater zone pulsing (extruder barrel 24 A, molder
      30 A) from the screw/pump running, so those two sim profiles read
      `on` during breaks whenever a zone is energised. Options: a minimum
      on-time / duty window before `on`, or a second (upper) threshold per
      device. Press, oven, CNC, saw, compressor, conveyor, welder, pump are
      fine with the single bar (`scripts/sim-fleet.sh threshold_for`).
      **Unblocks:** owner call on the semantics.
- [ ] Device `kind` on the record (extruder, press-brake, …) so the fleet
      table and device page can say what a machine is, seed the threshold
      default from it, and later pick per-kind charts. Today the only hint
      is the device id.
      **Unblocks:** the "producing" decision above (same record change).
- [ ] Live re-render cost: a live tab refetches and re-renders the whole
      page on every uplink (coalesced to ≥ 150 ms; ~2 renders/s with the
      current fleet). Fine for a few tabs; before a wall of dashboards,
      serve a `<main>` fragment or push bucket deltas over the SSE stream
      and patch client-side.
      **Unblocks:** more than a handful of concurrent live tabs.
- [ ] Threshold also on the `/fleet` summary tiles: "powered on ≥ N mA" on
      /ui/history still prints the fleet default; with per-device bars the
      caption should say "per-device thresholds" when any override exists.
      **Unblocks:** nothing — small follow-up.

## P4 — CI/CD & release

- [ ] Cut v0.2.0: everything since v0.1.0 (simulator + machine profiles,
      per-device power threshold, live windows, POE2 board, history API,
      light theme) ships only as hand-installed staging builds. Tag push is
      the owner's (`scratch/release.sh` on the Studio, `TAG=v0.2.0`), then
      `install-staging.sh TAG=v0.2.0`.
      **Unblocks:** owner tag push.

- [ ] Promote `.github/workflows/mutants.yml` (monthly cargo-mutants over
      endpoint + common, 456 mutants at the 2026-10-23 baseline) — scheduled
      workflows only run from the default branch, so it is inert on `dev`.
      First real run sizes the job; `--jobs 2` and a 6h ceiling are guesses.
      **Unblocks:** the next `dev` → `master` merge (owner).

## P5 — Enterprise track

See `todo/ENTERPRISE.md` (multi-table DynamoDB, billing sync, provisioning
workflows) and `ROADMAP.md` (when Step Functions and each orchestration layer
earn their keep — not before the enterprise features exist). Nothing here
blocks P0–P4.

## Switch day — LS108GP arrives (2026-10-26)

TP-Link LS108GP: 8× 802.3af/at, 30 W/port, 62 W total, power managed by
measured draw, unmanaged. Olimex boards pull a watt or two each.

- [ ] Extend Mode switch OFF (it drops ports 1-4 to 10 Mbps; the ESP32 EMAC
      wants 100 Mbps).
- [ ] Port 8 → house router (DHCP 10.0.0.x). Ports 1-7 → boards.
- [ ] POE2 first (`poe2-wifi-01`, MAC 10:97:bd:e5:9b:0c): it already runs the
      wired `board-poe2` firmware. Unplug USB *before* cabling — the POE2 has
      no galvanic isolation and Olimex says no Ethernet while USB is attached.
      Expect `poe2-wifi-01` fresh on https://staging.supervictor.advin.io/fleet
      with a new `last_uplink` and no `rssi_dbm`.
- [ ] POE-ISO (`poe-iso-eth-01`): `./scratch/flash.sh poe-iso-eth-01 staging`
      over USB (re-identify the port by MAC e0:5a:1b:66:50:40 — hub positions
      moved), then rack it. Isolated board, USB + Ethernet together is fine.
- [ ] Each additional board: `./Quickstart certs device <name>`, register
      (`POST /devices` with device_id/owner_id/subject_dn — `onboard` bundles it
      for Ethernet boards), add a `case` line to scratch/flash.sh (board →
      port, `board-poe2` or `ethernet`), flash, rack. Names by board type and
      position, e.g. `poe2-rack-01`.
- [ ] Fault test: pull one cable, watch the fleet row go stale → dark, plug it
      back, watch it recover. This is the baseline the mesh fallback has to beat.
- [ ] Then the coexistence spike: esp-radio ESP-NOW + ph-esp32-mac EMAC on one
      board (POE-ISO is the safest rig). Go/no-go for the mesh.

## Open questions for Nick

- 2026-10-24: mTLS identity never reaches the Lambda. dev.supervictor.advin.io exists (PR 74) and API Gateway enforces the client cert, but the Lambda Web Adapter serializes `lambda_http`'s typed request context, whose REST v1 identity struct has no `clientCert` (aws_lambda_events 0.14 and 1.2; `catch-all-fields` off), and REST parameter mapping needs a non-proxy integration. After PR 75 (no fallback to the client-supplied `x-ssl-client-subject-dn`, which anyone could forge on dev and prod) every Lambda request is anonymous, so bound devices 403 there. Pick one: (a) drop the adapter and run axum under `lambda_http` with `catch-all-fields`, read `identity.other["clientCert"]["subjectDN"]`; (b) move to an HTTP API v2, whose typed context carries `authentication.clientCert` through the adapter; (c) a REQUEST Lambda authorizer that copies `$context.identity.clientCert.subjectDN` into the authorizer context. I'd take (a): Rust-native, no sidecar, REST stays. Bench boards sit on staging (in-binary TLS, unaffected) until then.
- 2026-10-24: when real sensors land, does the telemetry simulator (`sim.rs` + the machine profiles in `profile.rs`) move behind a cargo feature (`sim`, off by default) so a production board can never report invented readings, or stay as the fallback for fields a board lacks? I'd put it behind a feature once the first real sensor ships (P2 item).
- 2026-10-24: 8-bit register levels (PR 85) fix each register's full scale in `supervictor/common/src/levels.rs`: current 0..6000 mA, temp -20..60 °C, humidity 0..100 %RH, rssi -100..-20 dBm, heap 0..512 KiB (1..=255 = range, 0 = no reading; 0.39 %/step). Changing a range later re-scales every stored burst, so confirm them before boards ship bursts for real: is 6 A the right current full scale for the sensor you have in mind (the sim profiles go to 60 A — those bursts pin at 255 today), and is -20 °C a low enough floor?
- 2026-10-24: default burst is 10 samples per 5 s uplink (`UPLINK_BURST`, 500 ms spacing; desktop `BURST=`). Capacity is 12 samples/uplink before the JSON buffer (768) needs to grow again. Keep 10, or go to 1 Hz (5 samples) to leave headroom for the IMU registers when they land? The burst is plain JSON bytes (`[12,200,…]`, ~3.5 chars/datapoint); base64 would be 1.33 but needs an encoder on the device — not worth it until the burst is the dominant byte count.
- 2026-10-25: heated machines — should `on` mean "producing" (needs a min-on-time or a second threshold per device) or stay "powered" (one bar, as shipped)? Extruder and molder are the only sim profiles where it matters (P3 item).
- 2026-10-25: mesh identity model — my pick: the PoE gateway's cert vouches for
  its ESP-NOW children (device record gains `gateway_id`), link encryption via
  a shared ESP-NOW PMK baked at flash time like SSID. Bench-grade, not
  per-leaf certs. Yes/no before the mesh protocol starts.
- 2026-10-25: board inventory for the rack — how many ESP32-POE2 vs
  ESP32-POE-ISO, and are any POE-ISO the WROVER variant (PSRAM → RMII clock on
  GPIO0 like the POE2; needs its own feature or a PSRAM flag)?
