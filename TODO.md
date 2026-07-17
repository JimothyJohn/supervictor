# TODO — Engineering Roadmap

Prioritized backlog of concrete improvements. The scaling plan lives in
`ROADMAP.md`; larger strategic tracks in `todo/` (ENTERPRISE.md,
open-source-framework.md). Completed items are deleted in the PR that lands
them — git history is the changelog. Every remaining item is gated; the
**unblocks** note says what has to happen first.

## P0 — Security & cost guardrails

- [ ] Repair the prod domain's truststore reference: `supervictor.advin.io`
      still points at the deleted temp key from a failed reload. One command
      (versioned pin, run as owner):
      `aws apigateway update-domain-name --domain-name supervictor.advin.io
      --patch-operations
      "op=replace,path=/mutualTlsAuthentication/truststoreUri,value=s3://supervictor/truststore.pem"
      "op=replace,path=/mutualTlsAuthentication/truststoreVersion,value=<current VersionId>"`
      (`Quickstart truststore-reload` does exactly this going forward).
      **Unblocks:** owner-run — API GW domain changes are gated.
- [ ] Billing alarm: CloudWatch `EstimatedCharges` alarm + SNS topic. Shares
      the topic/endpoint with ROADMAP Phase 2's dark-device alerting.
      **Unblocks:** an alert email/endpoint decision + account-level billing
      alerts enabled.
- [ ] Cert lifecycle: `deploy-staging.sh` signs with `certs/ca/ca.key` from
      the local working tree (gitignored, single copy on one laptop). Move
      signing material to SSM Parameter Store / Secrets Manager; document
      issuance + rotation; keep only public certs local.
      **Unblocks:** an ops window — touches live staging deploys.

- [ ] Bind uplink identity to the mTLS cert: `handle_uplink` trusts the
      payload's `id` and never compares it against the client cert's
      `subject_dn`, so any holder of ANY valid device cert can spoof any
      other device's uplinks. Enforce `client_subject` ↔
      `DeviceRecord.subject_dn` match when `require_registration` is on
      (registration already stores the DN); ship with a regression test
      that fails without the check.
      **Unblocks:** decision on legacy rows — registered devices with no
      `subject_dn` on record (reject vs grandfather-and-log).

## P1 — Hygiene

- [ ] Reconcile `.claude/settings.json` (added to this repo mid-session by a
      concurrent session while the same files were being deleted from the
      `~/.claude` config repo — since restored there). Decide which level owns
      the config and delete the other copy.
      **Unblocks:** owner adjudication.

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
      clean keepalive-expiry reconnects. Note: the `ethernet` feature is
      POE-ISO-specific — on the ESP32-POE2 the PHY doesn't respond at all
      (MDIO reads `0xffffffff`; different power/clock wiring). POE2 runs the
      `wifi` build; a `poe2` board module is a separate item if that board
      ever needs wired uplink.
      **Unblocks:** a hand on the cable (pull/replug is physical) and an
      hours-long soak window.
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
      buffering (by design); if live push matters there, that's the
      ECS-migration trigger per ROADMAP.md Phase 3.
      **Unblocks:** a real need for live push on the serverless path.
- [ ] Property tests (`proptest`) for payload validation.
      **Unblocks:** a dep review for the new dev-dependency.
- [ ] DynamoDB integration tests in CI (DynamoDB Local container or moto),
      so the `dynamo` feature is tested, not just compiled — and extend the
      store concurrency stress suite to it.
      **Unblocks:** container service in the CI job (docker-in-runner
      setup); no code blockers.

## P4 — CI/CD & release

- [ ] Periodic `cargo-mutants` run; mutation-catch rate is the real coverage
      number.
      **Unblocks:** nothing hard — needs a first baseline run to size the
      job (likely hours; schedule monthly, not per-PR).

## P5 — Enterprise track

See `todo/ENTERPRISE.md` (multi-table DynamoDB, billing sync, provisioning
workflows) and `ROADMAP.md` (when Step Functions and each orchestration layer
earn their keep — not before the enterprise features exist). Nothing here
blocks P0–P4.
