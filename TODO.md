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

## P1 — Hygiene

- [ ] `dev` branch protection contexts (owner-run, after `esp32-poe-iso`
      merges): required checks are still `Lint + test (host)` and
      `Embedded build (riscv32, ESP32-C3)`. esp32-poe-iso renames the
      embedded job to `Embedded build (ESP32 xtensa + ESP32-C3 riscv)` and
      the backend PR adds `DynamoDB store (DynamoDB Local)`; until the
      contexts match, PRs into dev wait forever on the old name and the
      dynamo job isn't required. `gh api -X PUT
      repos/JimothyJohn/supervictor/branches/dev/protection` per
      ~/.claude/templates/github/README.md.
      **Unblocks:** esp32-poe-iso landing (owner decision 2026-10-23).
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
- [ ] Point one bench board at the box (firmware bakes `HOST` at compile
      time → reflash with `HOST=staging.supervictor.advin.io`) so the
      running reference carries real uplinks, not just `Quickstart ping`.
      **Unblocks:** bench time.

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
## P4 — CI/CD & release

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
