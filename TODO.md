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
- [ ] **Flip staging to strict registration** — `REQUIRE_REGISTRATION=true` on the box (new boxes already default to it). The script lists devices without a bound certificate first and refuses unless the list is empty or `FORCE=1`; today `c3-wifi-01`, `c3-wifi-02`, `wrover-wifi-01` and `pi-edge-01` are unbound and dark — revoke them on /ui or re-register (`rack-flash.sh staging`, `Quickstart onboard`) first. Edits `/etc/supervictor/env` over SSM and restarts the unit (a restart is yours), then pings with a device cert and reads the summary.
      needs: a decision on the four unbound devices; a native terminal
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
- [ ] **Make the DynamoDB job a required check on `dev`** — the `DynamoDB store (DynamoDB Local)` job runs on every PR but is not required, so a dynamo-only failure cannot block auto-merge. The session's `gh api PATCH` was denied (CI-bypass class).
      needs: your gh login (repo admin)
      result: three required contexts on dev
      est: 2m
      difficulty: low
      id: dev-branch-protection
      asked: 2026-10-27
      by: decisions session
      notes: ./scripts/dev-branch-protection.sh
- [ ] **Put v0.2.0 on staging** — v0.2.0 is released (2026-10-28) and runs on prod; staging still runs whatever was hand-installed before it (nothing outside the box reports the version). `install-staging.sh` without a TAG installs the newest release.
      needs: a native terminal (the restart is yours)
      result: staging runs a reproducible release; the extruder/molder min-on can be applied (`./scripts/sim-fleet.sh thresholds staging`)
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
- [ ] **Restart the simulated fleet from a dev checkout and retire its worktree** — the ten `sim-*` containers still run from `../supervictor-sim-profiles` (an old branch, merged). The session was denied stopping them (workload interference). Thresholds for the eight unheated profiles were re-applied on staging 03:25Z; the extruder/molder 120 s min-on needs the new endpoint on the box first (cut-v0-2-0 or `scripts/deploy-staging-build.sh`).
      needs: Docker (up on the Studio); a checkout on dev
      result: containers built from dev; `git worktree remove ../supervictor-sim-profiles` possible
      depends: cut-v0-2-0
      est: 15m
      difficulty: low
      id: sim-fleet-restart
      asked: 2026-10-27
      by: decisions session
      notes: ./scripts/sim-fleet.sh down (in ../supervictor-sim-profiles), ./scripts/sim-fleet.sh up staging (from dev), ./scripts/sim-fleet.sh thresholds staging, then git worktree remove ../supervictor-sim-profiles
- [ ] **OTA resume proof not re-run against the current endpoint** — run 2 passed on rack-03 (revert after an EN reset mid-trial, resume after a power cut); the stalled-download proof (`OTA: resuming at`) was last run against an older endpoint build. Optional; the scripts live in `../supervictor-poe-iso/scratch/` (07, and 05 for the deterministic power-cut variant).
      needs: rack-03 on USB; ~5 min; the poe-iso worktree
      result: the resume path proven on the shipped endpoint, or the scripts deleted with the worktree
      options: run it | drop it
      est: 15m
      difficulty: low
      id: ota-resume-proof
      asked: 2026-10-27
      by: decisions session
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
- [ ] **Tear down the moot phone-preview stack in `../supervictor-mobile`** — endpoint :8443, extruder sim, demo http server on :8000; the session was denied the kill.
      needs: 1 min
      result: the ports and the worktree free
      est: 5m
      difficulty: low
      id: teardown-mobile-preview
      asked: 2026-10-26
      by: decisions session
      notes: ./scratch/teardown-mobile-preview.sh in the main checkout (untracked there)
- [ ] **Fill the switch-port column** — `certs/rack-inventory.tsv` still has `-` for every board, so a dark fleet row does not map to a cable (HARDWARE.md §10).
      needs: a look at the rack
      result: device → LS108GP port recorded
      est: 10m
      difficulty: low
      id: rack-switch-ports
      asked: 2026-10-27
      by: decisions session
- [ ] **Finish moving the boards to prod** — fresh on prod at 0.1.8 (2026-10-28 12:17Z): poe-iso-eth-01, poe-iso-rack-01, poe-iso-rack-03, poe2-wifi-01. Left: poe2-rack-01 (fresh on staging at 0.1.6, not yet registered on prod — rerun in progress) and poe-iso-rack-02 (registered on prod, dark on staging since 02:10:28Z; the 300 s watchdog did not recover it after the port-8 cable was re-seated — re-seat its own cable, then USB serial if still dark; it moves itself once it uplinks).
      needs: hands on rack-02; the esp toolchain; ~3 min per board
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

(none)
