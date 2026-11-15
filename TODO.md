# TODO — the board

Everything still open, in the board's item format
(`~/.claude/rules/todo.md`: `needs:` / `result:` / `options:` / `est:` /
`difficulty:` / `id:` lines; `/todo` renders this file). Direction with a
trigger lives in `ROADMAP.md`; decisions are recorded where they apply
(`HARDWARE.md` §11, `docs/MESH.md`, `docs/OTA.md`,
`supervictor/endpoint/CLAUDE.md`). Completed items are deleted in the PR
that lands them — git history is the changelog. An answered question is a
decision: act on it, then tick it in the change that resolves it.

Hand-off steps are tracked `scripts/` (each header says what it needs) and a
card here, never an untracked note; `scratch/` holds only throwaway output.

## Hands-on — Nick (gates an agent session cannot pass)

State as of 2026-10-28 ~12:30Z, after the prod cutover (PRs 131–133: the
Lambda stacks are gone, v0.2.0 is released, supervictor-endpoint-prod is up on
Let's Encrypt). `depends:` gives the order; the rest are independent.

- [ ] **Delete what the Lambda teardown left behind** — the two SAM ECR companion stacks (`supervictor-{dev,prod}-<hash>-CompanionStack`, ~500 MB of Lambda images), `s3://supervictor/truststore*.pem`, a CloudFront distribution still aliased to supervictor.advin.io with the deleted execute-api as its origin (outside any stack; the A record already points at the prod box, so it serves nothing), its ACM certificate, and the two ACM validation CNAMEs. The script disables CloudFront and waits before deleting (~15 min) and refuses if the origin is no longer execute-api.
      needs: a terminal with aws delete rights (denied to sessions); ~20 min
      result: no Lambda-era resource left in the account; `supervictor-endpoint-{staging,prod}` are the only supervictor stacks
      options: run it | keep some (say which)
      est: 20m
      difficulty: low
      id: teardown-lambda-leftovers
      asked: 2026-10-28
      by: dep-review session (docs/post-cutover-cleanup)
      notes: ./scripts/teardown-lambda-leftovers.sh — s3://supervictor/debian.cert.pem and supervictor.cert.pem are left alone: what are they?
- [ ] **Land the rewritten staging template on the live stack** — the bootstrap now installs a GitHub release (no Docker/Caddy), the IAM policy drops the bundle object, ReleaseTag/Repo/ExistingAllocationId params appear. Goes through `scripts/staging-stack-update.sh` (pins the live AMI, refuses a change set that replaces the instance); read the change set it prints before answering y. `FOLD_EIP=1` also hands the stack the out-of-band Elastic IP 100.56.171.238 — CloudFormation may fail to "delete" the stack's long-released ElasticIp resource and roll back; if so run once without it. The live stack was last updated 2026-10-25.
      needs: PR 122 on dev; a native terminal
      result: the stack matches the template in the tree; a rebuild would come up on the release install
      est: 20m
      difficulty: med
      id: staging-stack-changeset
      asked: 2026-10-27
      by: decisions session
      notes: TAG=v0.2.0 ./scripts/staging-stack-changeset.sh (YES=1 to skip the prompt)
- [ ] **Flip staging to strict registration** — `REQUIRE_REGISTRATION=true` on the box (new boxes already default to it). The script lists devices without a bound certificate first and refuses unless the list is empty or `FORCE=1`; on 2026-11-15 the list is empty (every staging device is bound). Edits `/etc/supervictor/env` over SSM and restarts the unit (a restart is yours), then pings with a device cert and reads the summary.
      needs: a native terminal
      result: only registered, active, certificate-bound devices can uplink to staging
      est: 15m
      difficulty: low
      id: staging-require-registration
      asked: 2026-10-27
      by: decisions session
      notes: ./scripts/staging-require-registration.sh
- [ ] **Billing alarm** — CloudWatch `EstimatedCharges` alarm on an SNS topic that emails you (the topic the dark-device alerting reuses, ROADMAP). Needs the console's Billing preferences → "Receive CloudWatch billing alerts" ticked once (no API); SNS sends a confirmation link.
      needs: EMAIL=…; THRESHOLD_USD (default 25); the billing-alerts preference
      result: an email when the month's estimate crosses the line
      est: 10m
      difficulty: low
      id: billing-alarm
      asked: 2026-10-27
      by: decisions session
      notes: EMAIL=you@… ./scripts/billing-alarm.sh
- [ ] **Put v0.2.0 on staging** — v0.2.0 is released (2026-10-28) and runs on prod; staging runs a hand-installed build (nothing outside the box reports the version; it does accept `min_on_s`, so it is at least the 2026-10-27 tree, and the sim thresholds including the 120 s min-on were applied 2026-11-15). `install-staging.sh` without a TAG installs the newest release.
      needs: a native terminal (the restart is yours)
      result: staging runs a reproducible release
      est: 10m
      difficulty: low
      id: cut-v0-2-0
      asked: 2026-10-26
      by: decisions session
      notes: TAG=v0.2.0 ./scripts/install-staging.sh
- [ ] **Promote dev → master** — your manual merge (master is 10 commits behind dev on 2026-10-28: PRs 131–133 and the cutover fixes); also activates `.github/workflows/mutants.yml` (scheduled workflows run from the default branch only) and publishes the docs site from the new tree.
      needs: whatever you want on dev
      result: master carries the single-binary reality; mutants run monthly
      est: 5m
      difficulty: low
      id: promote-dev-master
      asked: 2026-10-27
      by: decisions session
- [ ] **Release the admin-gated endpoint to staging and prod** — the device API (`POST /devices`, `GET /devices`, `GET /devices/{id}`, `GET /devices/{id}/uplinks`) is admin-only on dev since 2026-11-15; both boxes still run a build where any fleet-CA certificate passes. The scripts already register with the admin certificate, which the old build accepts too, so nothing breaks in between. Boards are untouched: firmware never calls these routes.
      needs: dev promoted to master; a native terminal (the restarts are yours)
      result: on both boxes a device certificate gets 403 from the device API (`.claude/skills/bring-your-own/scripts/check-server.sh <host>` shows the isolation line)
      depends: promote-dev-master
      est: 20m
      difficulty: low
      id: release-admin-gate
      asked: 2026-11-15
      by: board session (feat/admin-gate-device-api)
      notes: ./scripts/ship.sh go (tags, releases, installs prod), then ./scripts/install-staging.sh (staging, newest release)
- [ ] **Pi as a live device** — `pi-edge-01` is dark since 2026-10-24.
      needs: your password on nick@10.0.0.56 (no agent-reachable key is authorized); Docker if `target-pi/` needs a rebuild (`BUILD=1`)
      result: a Linux reference device fresh on staging
      est: 15m
      difficulty: low
      id: pi-edge-live
      asked: 2026-10-25
      by: decisions session
      notes: ./scripts/pi-edge.sh staging 12
- [ ] **Dashboard on the iPhone** — staging serves the glance UI; the phone needs the admin identity (Let's Encrypt server cert, no CA profile).
      needs: `~/.supervictor/` from browser-cert.sh on the Air (done 2026-10-24)
      result: https://staging.supervictor.advin.io/ui on the phone
      est: 10m
      difficulty: low
      id: iphone-dashboard
      asked: 2026-10-26
      by: decisions session
      notes: on the Air: scp nick@10.0.0.41:github/supervictor/scripts/iphone-cert.sh ~/ && ~/iphone-cert.sh (AirDrop the identity)
- [ ] **Fill the switch-port column** — `certs/rack-inventory.tsv` has ports for poe2-wifi-01 (1), poe-iso-eth-01 (2), poe2-rack-01 (3), poe-iso-rack-01 (5), poe-iso-rack-02 (6); poe-iso-rack-03 is still `-` (HARDWARE.md §10). The WiFi devkits have no port.
      needs: a look at the rack
      result: device → LS108GP port recorded
      est: 2m
      difficulty: low
      id: rack-switch-ports
      asked: 2026-10-27
      by: decisions session
- [ ] **Finish moving the boards to prod** — state 2026-11-15: fresh on prod at 0.1.9-rc.1: poe-iso-rack-01, poe2-wifi-01. Reached prod at 0.1.8 and dark since: poe-iso-eth-01 (2026-11-03), poe2-rack-01 (2026-11-01), poe-iso-rack-03 (2026-11-08, now running helmsman). Never seen on prod: poe-iso-rack-02 (registered; dark on staging since 2026-10-28 02:10Z — re-seat its cable, then USB serial; it moves itself once it uplinks). The next firmware must be newer than 0.1.9-rc.1 (anti-rollback).
      needs: hands on rack-02 and the two dark boards (power, cable); the esp toolchain; ~3 min per board
      result: the six PoE boards fresh on prod; staging keeps the simulated fleet
      est: 30m
      difficulty: med
      id: boards-to-prod
      asked: 2026-10-27
      by: prod-box session (PR 124)
      notes: ./scripts/boards-to-prod.sh (idempotent: skips boards already fresh on prod)

## Notes — local state (not tasks)

- Air keychain holds the admin identity + trusts the fleet CA (browser-cert.sh,
  2026-10-24). If the browser keeps prompting:
  `security set-key-partition-list -S apple-tool:,apple:,unsigned: -s ~/Library/Keychains/supervictor.keychain-db`
- Pi binary: `target-pi/aarch64-unknown-linux-gnu/release/supervictor-desktop`,
  built 2026-10-24 from PR 78; `BUILD=1 ./scripts/pi-edge.sh` rebuilds it.
- Staging stack updates: always through `scripts/staging-stack-update.sh`
  (pins `AmiId=/supervictor/staging/ami-id`; an unpinned update replaces the box).
- Boards pin `certs/ca/staging-bundle.pem` (fleet CA + ISRG Root X1), so the
  Let's Encrypt cert on staging (expires 2027-01-23, `certbot-renew.timer`)
  and a fleet-signed one both work. Never `supervictor-endpoint --version`
  on the box: it starts a second server and hangs an SSM script.
- Never leave `espflash monitor` attached across a reset (`espflash
  board-info` does the same): the CH340 parks the board in the ROM
  bootloader; `espflash reset --port <port>` recovers. `espflash reset` is
  a power-on reset and wipes the RTC OTA records (the trial count recovers
  from otadata; a download restarts from zero).
- Rack 2026-10-28: the PoE boards move to prod at 0.1.8 by OTA (card
  above); the WiFi devkits stay dark on staging on purpose; USB flash is
  recovery only. The LS108GP port-8 uplink cable has no latch — every board
  dark at once while the sims stay fresh means it slid out.

## Open questions for Nick

