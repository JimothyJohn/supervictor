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
      2026-10-25 with `scripts/deploy-staging-build.sh`:
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
- [x] OTA update path (2026-10-25, feat/ota): pull over the uplink's mTLS,
      A/B slots, sha256 + read-back verify, app-driven rollback after 3
      unconfirmed boots, `scripts/ota-push.sh`. docs/OTA.md "What shipped".
      Host-verified (577 tests incl. proptest fault schedules); bench
      verification is the open item below.
- [ ] **OTA bench verification** — first OTA on hardware PASSED 2026-10-25
      23:27Z: poe-iso-rack-03 (ethernet,espnow) 0.1.1 → 0.1.2 in 71 s from
      target set to the new version reporting; confirmed (39 min up, flat
      heap, no revert). The first 0.1.1 flash panicked on a 25.7 KB boot
      stack (fixed a11b7f6 + dram2 heap on every ESP32 build bc26475).
      Torture run 1 (2026-10-26 01:08Z) FAILED usefully: 04 passed falsely at
      +2 min (it read the stale pre-OTA fleet row as "back on 0.1.2"), so 05
      ran while 0.1.3 was in its trial boots and its `espflash reset` — a
      power-on reset, on which esp-hal zeroes `persistent` RTC statics — wiped
      the boot record; the firmware took 0.1.3 as confirmed and sat dark for
      an hour. Fixed on feat/ota: boot_check rebuilds the trial from otadata
      (`New` slot + no record → Pending), 04 demands a fresh uplink and can
      EN-reset mid-trial (`EN_RESET_AT`), 05 says what a power-on reset can
      show (restart, not resume). Rerun order, scripts in the poe-iso worktree:
      `scratch/03-install-staging-ota.sh` (yours: re-install the feat/ota
      endpoint, it gained the `firmware_fault` route), `scratch/06-ota-reflash-rack-03.sh`
      (yours: USB flash 0.1.5), then `EN_RESET_AT=150 scratch/04-ota-revert-test.sh`
      (~17 min), `scratch/05-ota-resume-test.sh` (~4 min, power-cut restart),
      `scratch/07-ota-resume-proof.sh` (~4 min, stalled download → 'OTA: resuming
      at'); then 20 self-swaps. Never leave
      a serial monitor attached across a reset: the CH340 parks the board in
      the ROM bootloader (happened 01:52Z; `espflash reset` recovers).
      needs: a board on USB (rack-03 is, stuck on wrong-CA 0.1.3 until 06 runs); ~30 min of bench time
      result: OTA proven on hardware; the fleet can be reflashed once over USB and never again
      est: 2h
      difficulty: med
      id: ota-bench
- [ ] OTA: S3-backed firmware store for the Lambda stages (dev/prod answer
      503 on the firmware routes until then); signed manifests (O.2);
      identity in NVS so one image serves the fleet. docs/OTA.md.
      **Unblocks:** ota-bench green.

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
- [x] Threshold also on the `/fleet` summary tiles: "powered on ≥ N mA" on
      /ui/history still prints the fleet default; with per-device bars the
      caption should say "per-device thresholds" when any override exists.
      **Unblocks:** nothing — small follow-up.
      notes: done 2026-10-26 (feat/history-threshold-caption): the tiles say "per-device thresholds — n of m set, fleet default N mA" once any active device overrides; `/fleet/summary` gains `threshold_overrides`

## P4 — CI/CD & release

- [ ] Cut v0.2.0: everything since v0.1.0 (simulator + machine profiles,
      per-device power threshold, live windows, POE2 board, history API,
      light theme) ships only as hand-installed staging builds. Tag push is
      the owner's (`TAG=v0.2.0 ./scripts/release.sh`), then
      `TAG=v0.2.0 ./scripts/install-staging.sh`.
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
- [x] POE2 first (`poe2-wifi-01`, MAC 10:97:bd:e5:9b:0c) — racked on switch port 1, 2026-10-27 01:45Z, fresh on staging: it already runs the
      wired `board-poe2` firmware. Unplug USB *before* cabling — the POE2 has
      no galvanic isolation and Olimex says no Ethernet while USB is attached.
      Expect `poe2-wifi-01` fresh on https://staging.supervictor.advin.io/fleet
      with a new `last_uplink` and no `rssi_dbm`.
- [x] POE-ISO (`poe-iso-eth-01`) — racked on switch port 2, 2026-10-27 02:34Z: `./scripts/flash.sh poe-iso-eth-01 staging`
      over USB (re-identify the port by MAC e0:5a:1b:66:50:40 — hub positions
      moved), then rack it. Isolated board, USB + Ethernet together is fine.
- [x] **A/B reflash of the rack (the last USB flash)** — DONE 2026-10-27 01:45–02:37Z: six PoE boards flashed from dev 43fd325 with FW_VERSION=0.1.6 baked, all fresh on staging at 0.1.6 (ports: poe2-wifi-01 →1, poe-iso-eth-01 →2, poe2-rack-01 →3, poe-iso-rack-01 →5, poe-iso-rack-02 →6; rack-03 was already on the table). The two WiFi devkits (c3-wifi-01, wrover-wifi-01) stay on 0.1.0 by Nick's call. rack-flash.sh fixes from the run: PR fix/rack-flash-robust. Decided 2026-10-26:
      once PR 105 is on dev, every board gets `./scripts/rack-flash.sh staging`
      (or `flash.sh <device> staging`) from a checkout on dev so it carries
      `partitions.csv`; after that OTA (`scripts/ota-push.sh`) is the only
      update path and the table is frozen (`ota::partitions_tests`).
      needs: PR 105 merged (ota-bench green); boards on USB one at a time (~2 min each)
      result: seven boards on the A/B table; rack-flash.sh is the recovery path only
      depends: ota-bench
      est: 1h
      difficulty: low
      id: ab-reflash-rack
- [ ] Each additional board: `./scripts/rack-flash.sh staging` does the whole
      loop (cert, `POST /devices`, flash, cabling prompt, uplink poll) and
      records it in `certs/rack-inventory.tsv`. Names by board type and
      position, e.g. `poe2-rack-01`.
- [x] Fault test (2026-10-25): pulled poe-iso-eth-01's cable at 18:46Z, the
      row went stale at 19:01Z (FRESH_SECS = 15 min) and was fresh again 20 s
      after the cable went back. Dark (2 h) not exercised. Baseline for the mesh.
- [x] Coexistence spike (2026-10-25): **GO.** `--features ethernet,espnow`
      (src/network/espnow.rs) on poe-iso-rack-02 + -03: 29/29 beacons heard,
      no gaps, both boards uplinking every 5 s throughout, all six rack rows
      fresh. Cost: the radio blob is ~34 KB of DRAM, which overflowed the boot
      stack at the 112 KB heap; the espnow build runs 80 KB + a 96 KB second
      heap in `dram2_seg` (esp-hal `.dram2_uninit`), 92 KB used / 88 KB free.
      dram2 heap promoted to every classic-ESP32 build in feat/ota (22:45Z,
      after a 3 h soak on rack-02/03; the OTA statics had squeezed the WiFi
      build's boot stack to 26 KB, now 59-61 KB). Next: the mesh identity
      answer (open question below), then the ESP-NOW protocol (gateway/leaf
      roles, PMK) on top of this module.

## Open questions for Nick

- [ ] **mTLS identity never reaches the Lambda** — dev.supervictor.advin.io exists (PR 74) and API Gateway enforces the client cert, but the Lambda Web Adapter serializes `lambda_http`'s typed request context, whose REST v1 identity struct has no `clientCert` (aws_lambda_events 0.14 and 1.2; `catch-all-fields` off), and REST parameter mapping needs a non-proxy integration. After PR 75 (no fallback to the client-supplied `x-ssl-client-subject-dn`, which anyone could forge on dev and prod) every Lambda request is anonymous, so bound devices 403 there. (a) drop the adapter and run axum under `lambda_http` with `catch-all-fields`, read `identity.other["clientCert"]["subjectDN"]`; (b) move to an HTTP API v2, whose typed context carries `authentication.clientCert` through the adapter; (c) a REQUEST Lambda authorizer that copies `$context.identity.clientCert.subjectDN` into the authorizer context. I'd take (a): Rust-native, no sidecar, REST stays. Bench boards sit on staging (in-binary TLS, unaffected) until then.
      needs: a letter
      result: bound devices authenticate on dev and prod, not just staging
      options: a lambda_http catch-all | b HTTP API v2 | c REQUEST authorizer
      est: 1d
      difficulty: med
      id: lambda-mtls-identity
      asked: 2026-10-24
      answer: Why are we still using AWS Lambda when we can run this on our own naked infra anywhere?
- [ ] **Telemetry simulator behind a cargo feature once real sensors land?** — `sim.rs` + the machine profiles in `profile.rs` are compiled in unconditionally and fill temp/humidity/rssi with plausible fakes. Move behind `sim` (off by default) so a production board can never report invented readings, or keep as the fallback for fields a board lacks? I'd put it behind a feature once the first real sensor ships (P2 item).
      needs: yes/no
      result: a production build cannot invent readings
      options: feature off by default | keep as fallback
      est: 2h
      difficulty: low
      id: sim-behind-feature
      asked: 2026-10-24
      answer: feature off by default
- [ ] **Is -20 °C a low enough temperature floor?** — 8-bit register levels (PR 85) fix each register's full scale in `supervictor/common/src/levels.rs`: temp -20..60 °C, humidity 0..100 %RH, rssi -100..-20 dBm, heap 0..512 KiB (1..=255 = range, 0 = no reading; 0.39 %/step). Current was answered 2026-10-25 (20 A, the SEN0211 probe range).
      needs: a floor in °C
      result: the temp register scale is final before boards ship
      options: -20 | -40 | other
      est: 30m
      difficulty: low
      id: temp-floor
      asked: 2026-10-24
      answer: -40
- [ ] **Heated machines: does `on` mean producing or powered?** — "producing" needs a min-on-time or a second threshold per device; "powered" is one bar, as shipped. Extruder and molder are the only sim profiles where it matters (P3 item).
      needs: a definition
      result: the power state matches what an operator means by "on"
      options: powered (as shipped) | producing (second threshold) | producing (min-on-time)
      est: 3h
      difficulty: med
      id: heated-on-meaning
      asked: 2026-10-25
      answer: producing (min-on-time)
- [ ] **Mesh identity model: gateway vouches for its ESP-NOW children?** — my pick: the PoE gateway's cert vouches for its ESP-NOW children (device record gains `gateway_id`), link encryption via a shared ESP-NOW PMK baked at flash time like SSID. Bench-grade, not per-leaf certs. The coexistence spike is a GO (PR 103); the mesh protocol starts on this answer.
      needs: yes/no
      result: the ESP-NOW protocol (gateway/leaf roles, PMK) gets built on the spike module
      options: yes, gateway vouches + shared PMK | no, per-leaf certs | other
      est: 2d
      difficulty: high
      id: mesh-identity-model
      asked: 2026-10-25
      by: coexistence-spike session (feat/espnow-coexist)
      answer: yes, gateway vouches + shared PMK. I would love for the moth module to be connected to the network with the rest of them simply hunting for signal. The idea would be they have external antennas which can be stored in the open space, not in an enclosure.
- [ ] **Board inventory for the rack** — how many ESP32-POE2 vs ESP32-POE-ISO, and are any POE-ISO the WROVER variant (PSRAM → RMII clock on GPIO0 like the POE2; needs its own feature or a PSRAM flag)? rack-flash.sh refuses a WROVER POE-ISO until this is known.
      needs: counts, and WROVER yes/no
      result: every rack board has a verified board feature
      options: no WROVER | some WROVER (count)
      est: 1h
      difficulty: low
      id: rack-board-inventory
      asked: 2026-10-25
      answer: They're all WROOM but I like being prepared for other possible platforms with some kind of hardware abstraction-layer that's application-specific as crazy as that may be.
- [x] **Which local-preview path failed on the new phone UI?** — PR 96 (phone glance layer) is on dev. The mobile-UI session left a Quickstart stack running in `../supervictor-mobile` (endpoint :8443 with a multi-name server cert, extruder sim, `.logs/`), a demo http server on :8000 serving `scratch/demo` + `supervictor-ca.pem`, and `scratch/01-trust-fleet-ca.sh` for the Studio's keychain. Nick said "not working" without saying which step: Studio browser (CA trust not yet run; server cert verified by curl on every address), the phone's CA profile / full-trust toggle, the phone identity (.p12 from 2026-10-24), or the phone being on 10.0.0.x rather than 192.168.3.x. Full state in that worktree's `.claude/handoff.md`.
      needs: the exact symptom (which URL, which device, what error text)
      result: a working preview path written into scripts/ or the README, and the stack torn down
      options: Studio browser | phone CA trust | phone identity | network | something else
      est: 1h
      difficulty: low
      id: mobile-preview-failed
      asked: 2026-10-25
      by: mobile-UI session (feat/ui-mobile-glance)
      answer: the phone couldn't access the page
      notes: resolved 2026-10-26: moot — PR 96 is on dev and staging already serves the glance UI, so the phone path is staging + scripts/iphone-cert.sh (Let's Encrypt cert, no CA profile; AirDrop the identity). The local stack in ../supervictor-mobile is torn down by scratch/teardown-mobile-preview.sh (yours; kill was denied to me)
- [x] **Kit power threshold: default 1000 mA for clamp devices?** — `POWER_THRESHOLD_MA` defaults to 100 mA, below the SEN0211 clamp's ~1 A ADC floor (HARDWARE.md §6.2), so a kit device reads "on" from noise. Per-device `power_threshold_ma` already exists on the record.
      needs: a number, or "per device only"
      result: kit devices show off when the machine is off; the default documented in HARDWARE.md and the endpoint config
      options: 1000 mA default | keep 100, set per device | other number
      est: 30m
      difficulty: low
      id: kit-power-threshold
      asked: 2026-10-25
      by: hardware session (docs/hardware-spec, feat/current-register-20a)
      answer: 1000 mA default
      notes: done 2026-10-26 (chore/decisions-20261026): fleet::DEFAULT_POWER_THRESHOLD_MA = 1000; HARDWARE.md §6.2/§11, server.md, endpoint CLAUDE.md updated; sim profiles keep their own thresholds
- [x] **Clamp calibration gain: compile-time or NVS?** — one-point gain per board. Compile-time `CURRENT_GAIN_PPM` baked like `UPLINK_INTERVAL_S` (simple, a reflash to change, a column in certs/rack-inventory.tsv) or NVS (survives OTA, needs a write path). My pick: compile-time for v0. HARDWARE.md §7, §11.
      needs: your pick
      result: the firmware's `sensor-current` feature has one calibration path, not two
      options: compile-time env | NVS | compile-time now, NVS after OTA
      est: 2h
      difficulty: low
      id: clamp-calibration-location
      asked: 2026-10-25
      by: hardware session (docs/hardware-spec)
      answer: compile-time env
      notes: done 2026-10-26: HARDWARE.md §6.2/§7/§11 name CURRENT_GAIN_PPM as the only path; the firmware feature itself (sensor-current) is still to be written and will read it like UPLINK_INTERVAL_S
- [x] **Prototype scope to RFQ: hat only, or hat + integrated node?** — SV-HAT-A (`hardware/hat/`, ~$0.5–5k, 6–10 weeks) vs also the CERN-OHL derivative of the Olimex POE-ISO (~$12–30k, 12–20 weeks). Recommendation: hat now, node at 25+ units. hardware/SOURCING.md §1, §5.
      needs: your pick, then the bracketed parts of hardware/RFQ-prototype.md §12
      result: RFQ sent to three shops; quotes replace the blog-number estimates
      options: hat only | hat + node | neither yet
      est: 2h
      difficulty: low
      id: rfq-scope
      asked: 2026-10-25
      by: hardware session (docs/hardware-spec)
      answer: hat only
      notes: done 2026-10-26: RFQ-prototype.md is hat-only (Scope C kept as context, cover email rewritten); SOURCING.md §1/§5 and hardware/index.html record the decision. Still yours: the bracketed parts of §12 and sending it
- [x] **OTA enablement: when does the rack take its one USB reflash?** — PR 105 (feat/ota) changes the partition table to the A/B layout; merging is safe (boards on the factory table log `OTA: unavailable` and keep uplinking) but each board needs one more USB flash (`./scripts/flash.sh <device> staging`, ~2 min each) before it can ever OTA. Seven racked boards today. After that, never again.
      needs: a date, or "merge now, flash as boards come to the bench"
      result: PR 105 un-drafted; every board on the A/B table; rack-flash.sh becomes the recovery path only
      options: switch day 2026-10-26, all seven | merge now, flash opportunistically | hold until the bench run (ota-bench) passes
      est: 30m
      difficulty: low
      id: ota-enablement-reflash
      asked: 2026-10-25
      by: OTA session (feat/ota)
      answer: That's fine, we can flash them all via USB again, but please never again.
      notes: decided 2026-10-26: one more USB flash of every board, then never again — partitions.csv is frozen (ota::partitions_tests pins it, docs/OTA.md "Contract"). Sequence: 03 → 06 → 04 → 05 → 07 on rack-03, un-draft PR 105, then rack-flash.sh from a checkout on dev for the other six (switch-day card ab-reflash-rack)
- [x] **OTA resume: must a download survive a power cut, or only a reset?** — the resume record (offset into the inactive slot) lives in RTC fast RAM, which esp-hal zeroes on every power-on/EN reset by design; it survives the 5-min watchdog and software resets. So a PoE switch reboot or power cut mid-download restarts the ~60 s pull from zero (correct, just not a resume), and nothing on the bench can trigger a record-keeping reset from outside, so 'OTA: resuming at' has no hardware proof yet. My pick: accept restart-on-power-cut (one minute, no flash wear), and prove resume by letting the download run past the watchdog once (`UPDATE_DEADLINE` is 200 s inside a 300 s watchdog, so a deliberately slow image: an endpoint knob to throttle one device's image bytes) rather than adding a flash-resident record or a reset endpoint on the board.
      needs: your pick
      result: docs/OTA.md states the contract; 05 (or a 07) proves the path that is promised
      options: accept restart on power cut, prove resume via throttled download | resume must survive power cut (flash-resident record) | drop resume, always restart
      est: 2h
      difficulty: med
      id: ota-resume-proof
      asked: 2026-10-26
      by: OTA session (feat/ota)
      answer: accept restart on power cut, prove resume via throttled download
      notes: decided 2026-10-26: power cut = restart from zero; resume proven via a one-shot server stall (`PUT /devices/{id}/firmware_fault`, feat/ota) — `scratch/07-ota-resume-proof.sh` after the staging endpoint is re-installed (03). docs/OTA.md "Contract and bench proof"
