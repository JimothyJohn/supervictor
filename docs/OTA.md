# OTA Updates — Roadmap

Expands the Phase-2 OTA bullet in `ROADMAP.md`. Written July 2026 against
commit `98a80c1` (telemetry wire schema v2). First target: **the PoE boards**
— stable power (no brownout mid-flash-write), wired uplink (a ~1 MB image is
seconds, not minutes), and `poe-iso-eth-01` is the hardware-verified
reference board.

## The model: pull, never push

There is no "flash a device at IP X." Devices are DHCP/NAT'd, run no
listener, and an inbound flash port is an RCE surface. The architecture
stays outbound-only mTLS: the cloud records a *desired* firmware version per
device; the device notices on its next uplink and pulls the image over the
channel it already trusts. "Flash that device" becomes "set `desired_fw` on
its row" — it takes effect within one uplink interval (now self-reported per
uplink as `interval_s`, so `/fleet` can show exactly how stale a device can
be).

Everything below rides on what already exists: mTLS identity per device,
fw version reported in every uplink (v2 schema), `/fleet` staleness API,
and the `chip-esp32 / chip-esp32c3` build axis.

## Phase O.0 — A/B boot groundwork (bench only, no cloud changes)

Prove the swap mechanics over serial before any network is involved.

- New `partitions_ota.csv`: `nvs / phy_init / otadata (0x2000) / ota_0 /
  ota_1`. 4 MB flash → two ~1.9 MB (`0x1E0000`) app slots; current debug
  image is ~1.0 MB, release smaller. Add a CI check that the built image
  fits a slot with headroom — silent slot overflow is a bricked rollout.
- Add `esp-storage` (esp-rs flash driver implementing the embedded-storage
  traits `FlashRegion` needs). Only new dependency in the whole plan;
  `esp-bootloader-esp-idf` 0.5 is already in-tree and provides `OtaUpdater`
  (inactive-slot write region, `activate_next_partition()`, ESP-IDF
  `otadata` state machine).
- Bench milestone: boot from `ota_0`, write a second image to `ota_1` *from
  the running firmware*, activate, reboot into it, and confirm the
  `PendingVerify → Valid` handshake. Then the torture tests: power cut
  mid-write (must reboot into old image), corrupt image activated (must
  roll back via `Aborted`).

**Exit criteria:** a POE-ISO on the bench survives 20 self-swaps and 5
mid-write power cuts with zero bricks.

## Phase O.1 — Pull transport over existing mTLS

- **Control channel:** add `desired_fw` to the devices table; the uplink
  *response* (currently discarded) carries it back. No polling endpoint, no
  new connection — the 5-second heartbeat is the poll.
- **Artifact route:** `GET /firmware/{version}/{chip}` on the endpoint
  (device mTLS, same axum binary both paths) — S3-backed on Lambda,
  disk-backed self-hosted. Supports `Range`: the device's heapless response
  buffer is 1 KB, so the image streams down in ranged chunks written
  straight to the inactive slot.
- **Device loop:** running fw ≠ desired fw → chunked download → SHA-256
  over the full slot vs manifest → activate → reboot. Uplink loop stays
  alive during download (it's the health signal).
- **Confirm = first successful mTLS uplink after boot** → set
  `OtaImageState::Valid`. Can't reach the cloud on new firmware? Bootloader
  rolls back on next watchdog reboot. The device's definition of "working"
  is the only one that matters: can it phone home.

**Exit criteria:** bench POE-ISO updated end-to-end by flipping
`desired_fw`, including one deliberately-broken image that auto-rolls back
and reports the old version to `/fleet`.

## Phase O.2 — Authenticity & anti-rollback

mTLS says the *server* is ours; it does not make an S3 object or a
compromised endpoint box trustworthy. Images authenticate themselves:

- **Signed manifests** (ed25519): `{version, chip, size, sha256, sig}`.
  Public key baked into firmware; private key lives in KMS/SSM, CI signs at
  release. Device verifies before activating, full stop.
- **Anti-rollback:** monotonic version compare on-device; downgrades require
  an explicitly-flagged signed manifest (the escape hatch for a bad
  release), and a replayed old manifest is refused.
- **Adversarial tests, per house rules:** fuzz the manifest parser
  (`cargo-fuzz` — external bytes), wrong-chip image, oversized image,
  truncated download resumed across reboot, torn otadata write, replayed
  manifest, sig-stripped manifest. Every one is a regression test before
  the fix, kept forever.

## Phase O.3 — Fleet rollout mechanics

- **Release pipeline:** tag → CI builds per chip (existing feature axis) →
  signs → uploads artifact + manifest to S3 → registers version. No
  local-build artifacts ever ship (existing deploy-chain rule).
- **Staged rollout:** `desired_fw` is per-device, so canary is free — bench
  rig first, then percentage waves. `/fleet`'s fw column is the rollout
  dashboard; the staleness watchdog is the tripwire (a wave that goes dark
  halts the rollout).
- **Cert rotation** (per `ROADMAP.md` / `todo/ENTERPRISE.md`) rides the
  same channel once it exists: new cert delivered like a firmware blob,
  swap, confirm, revoke old.

## Sequencing & triggers

O.0 is buildable now and is pure bench derisking. O.1 needs the endpoint
artifact route (small). O.2 before *any* non-bench device updates — signing
is not optional at fleet scale. O.3 when the Phase-2 trigger fires (>1 site
or >10 devices). WiFi boards (POE2, C3s) inherit everything; they just add
brownout risk during writes, so they follow after the PoE path has soak
time.

## Open questions

- Uplink response schema: piggyback `desired_fw` on the existing 2xx body
  vs a versioned response envelope (ties into wire schema v2 work).
- Where version state lives self-hosted (SQLite column mirrors the Dynamo
  field — store trait already abstracts this).
- espflash-side tooling: keep `Quickstart onboard` serial path as the
  factory/recovery flow; OTA never replaces it.
