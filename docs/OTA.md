# OTA Updates

Expands the Phase-2 OTA bullet in `ROADMAP.md`. Written July 2026 against
commit `98a80c1` (telemetry wire schema v2); **implemented 2026-10-25**
(O.0 + O.1, plus the digest and anti-rollback pieces of O.2) — see "What
shipped" below, then the original plan. First target: **the PoE boards**
— stable power (no brownout mid-flash-write), wired uplink (a ~1 MB image is
seconds, not minutes), and `poe-iso-eth-01` is the hardware-verified
reference board.

## What shipped (2026-10-25)

Operator view, staging:

```sh
FW_VERSION=0.2.0 ./scripts/ota-push.sh poe-iso-rack-03 staging
```

builds that device's image, uploads it (`PUT /devices/{id}/firmware/0.2.0`,
admin mTLS), sets `desired_fw` (`PUT /devices/{id}/desired_fw`), and polls
`/fleet` until the `fw` column flips. `fw_pending` on the fleet row is the
rollout dashboard. The whole path is per device because every image bakes
in that device's certificate and key.

Device view (`supervictor/edge/src/ota/`):

1. Every uplink response carries `desired_fw` when an operator set one.
   The device compares it with `FIRMWARE_VERSION` (`FW_VERSION` at build
   time, else the crate version) and only moves to a strictly newer
   version — a replayed or stale target is ignored (anti-rollback).
2. It fetches `…/firmware/{version}/manifest` (`version`, `size`,
   `sha256`, `chip`), refuses another chip or another version, then opens
   one `GET …/firmware/{version}` with `Range: bytes=<resume>-` and
   `If-Match: "<sha256>"` (a re-upload mid-download is a 412, not a splice).
3. The body streams into the inactive A/B slot in 4 KB sectors; the ESP-IDF
   header is checked at the first sector, SHA-256 runs as it goes, and
   `finish` compares size + digest and then **reads the slot back** and
   hashes it again. Progress (sector-aligned offset + digest prefix) lives
   in RTC memory, so a reset mid-download resumes from flash that is really
   there — the watchdog is deliberately *not* fed during a download.
4. Activate (`otadata` → the new slot, state `New`), record `Pending`
   with the previous slot, reboot.
5. **Rollback is app-driven.** The bootloader espflash ships selects slots
   but never changes `New → PendingVerify → Aborted`, so this firmware
   counts boots of an unconfirmed image in a checksummed RTC record: the
   first 2xx uplink marks the slot `Valid`; the third unconfirmed boot
   flips `otadata` back and resets. With the 5-minute RTC watchdog that
   bounds a bad image at ~15 minutes.

Partition table (`supervictor/edge/partitions.csv`): `nvs / otadata /
phy_init / ota_0 / ota_1`, two 0x1F0000 slots; `espflash` writes `ota_0`
and `scripts/flash.sh` erases `otadata` on every USB flash. A board still
on the factory table logs `OTA: unavailable` and keeps uplinking; **one USB
flash per board enables OTA** (`./scripts/flash.sh <device> staging`).

Endpoint (`supervictor/endpoint/src/firmware.rs`): `FIRMWARE_DIR` on disk,
content-addressed blobs beside a manifest (atomic rename, the manifest is
the commit marker), image validation on upload (magic, chip, slot fit,
appended sha256), `Range` + `If-Match` on download. Download is allowed to
the device's own bound certificate or an admin only. Without
`FIRMWARE_DIR` (the Lambda stages today) the routes answer 503.

Tests: `supervictor/edge/src/ota/*_tests.rs` (proptest fault schedules:
random chunking, resets at arbitrary points, byte corruption, overrun,
lying flash; request-smuggling shapes; random RTC memory never reverting)
and `supervictor/endpoint/tests/test_firmware_api.rs` (another device's
certificate, unbound devices, traversal, torn/oversize/wrong-chip images,
every `Range` shape, concurrent re-uploads against readers).

Not yet: signed manifests (O.2 signing), an S3 store for the Lambda
stages, generic images with the identity in NVS (would make one image
serve the fleet), staged rollouts (O.3). The bench exit criteria in O.0
and O.1 below are the next thing to run.

## Contract and bench proof (2026-10-26)

- **A power cut is a restart, not a resume.** Both RTC records (trial-boot
  count, download progress) live in `#[ram(persistent)]` memory, which
  esp-hal zeroes on a power-on reset — a power cycle, the EN pin,
  `espflash reset`, a PoE switch reboot. A board power-cycled mid-download
  pulls the image again from zero (~60 s on PoE). A board power-cycled
  during its trial boots rebuilds the trial from otadata
  (`policy::adopt_untracked`): a slot still `New` with no record is on
  trial, the other slot is the fallback. Watchdog (`SysRtcWdt`) and
  software resets keep both records. Decided 2026-10-26 (TODO
  `ota-resume-proof`): no flash-resident resume record.
- **Resume is proven without a reset.** `PUT /devices/{id}/firmware_fault`
  (admin, body `{"stall_at": N, "stall_secs": 15}`, one-shot, in memory)
  makes the device's next image download pause after `N` bytes for longer
  than its 10 s read timeout. The device drops the connection, keeps its
  resume record, and the next uplink retries with `Range` + `If-Match`:
  serial prints `OTA: resuming at N'` and the fleet reports the new version.
  `scratch/07-ota-resume-proof.sh` runs it on rack-03; the endpoint tests
  pin the bytes-and-framing invariant.
- **The partition table is frozen.** `supervictor/edge/partitions.csv`
  (nvs / otadata / phy_init / ota_0 / ota_1 on 4 MB) changes only with a USB
  flash of every board. The rack takes that flash once, after PR 105 lands;
  `ota::partitions_tests` pins the layout so the next change is deliberate.
- **Bench rules.** Never leave a serial monitor attached across an expected
  reset (CH340 + GPIO0 strap → ROM bootloader); treat `espflash reset` as a
  power cut in any test design; the revert test passes only on a *new*
  uplink of the old version.

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
  `OtaImageState::Valid`. Can't reach the cloud on new firmware? The
  firmware itself rolls back after three unconfirmed boots (the espflash
  bootloader has no rollback — see "What shipped"). The device's
  definition of "working" is the only one that matters: can it phone home.

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
