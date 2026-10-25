#!/usr/bin/env bash
# ota-push.sh — build a device's firmware, upload it to the endpoint, set it as
# the device's target, and watch the device come back on it (docs/OTA.md).
#
#   FW_VERSION=0.2.0 ./scripts/ota-push.sh <device> staging [--no-activate] [--no-wait]
#
#   device        DEVICE_NAME baked into the image (certs/devices/<device>/ must
#                 exist; the image carries that device's cert + key, which is why
#                 images are per device and only that device's cert may fetch one).
#   staging       the only target with a firmware blob store today (FIRMWARE_DIR on
#                 the box). dev/prod run on Lambda without one (TODO.md).
#   FW_VERSION    version the image reports and the manifest is filed under;
#                 MAJOR.MINOR.PATCH[-pre], and it must be NEWER than what the
#                 device runs or the device ignores it (anti-rollback). Required.
#   FEATURES      as flash.sh: default from certs/rack-inventory.tsv.
#   FW_ROOT       checkout to build from (default: this one).
#   --no-activate upload only; set desired_fw later with:
#                 admin curl -X PUT .../devices/<device>/desired_fw -d '{"desired_fw":"X"}'
#   --no-wait     do not poll /fleet for the device to report the new version.
#   ADMIN         admin certificate (default $USER).
#
# What the device does with it: on its next uplink the response carries
# desired_fw; it fetches the manifest, streams the image into its inactive slot,
# verifies size + sha256, reboots into it, and confirms on its first uplink
# (three unconfirmed boots roll it back). Expect the fw column to flip within
# about one uplink interval plus the download (~30-60 s on PoE).
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
# shellcheck disable=SC2034  # read by _lib.sh's log()
LOG_TAG=ota
USAGE="usage: FW_VERSION=X.Y.Z ota-push.sh <device> staging [--no-activate] [--no-wait]"
DEVICE="${1:?$USAGE}"
TARGET_ENV="${2:?$USAGE}"
shift 2
ACTIVATE=1; WAIT=1
for arg in "$@"; do
    case "$arg" in
    --no-activate) ACTIVATE=0 ;;
    --no-wait) WAIT=0 ;;
    *) fail "unknown flag $arg — $USAGE" ;;
    esac
done
FW_VERSION="${FW_VERSION:?set FW_VERSION, e.g. FW_VERSION=0.2.0 (must be newer than the fw the device runs)}"
FW_ROOT="${FW_ROOT:-$REPO_ROOT}"
INVENTORY="${INVENTORY:-$REPO_ROOT/certs/rack-inventory.tsv}"
need cargo espflash curl python3 shasum
need_admin
[ "$TARGET_ENV" = staging ] || fail "only staging has a firmware store today (dev/prod are Lambda; TODO.md)"
[ -d "$FW_ROOT/supervictor/edge" ] || fail "no firmware checkout at $FW_ROOT"
[ "$FW_ROOT" = "$REPO_ROOT" ] || [ -e "$FW_ROOT/certs" ] || ln -s "$REPO_ROOT/certs" "$FW_ROOT/certs"
[ -f "$REPO_ROOT/certs/devices/$DEVICE/client.pem" ] || fail "no device cert for $DEVICE"
echo "$FW_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' || fail "FW_VERSION '$FW_VERSION' is not MAJOR.MINOR.PATCH[-pre]"

if [ -z "${FEATURES:-}" ] && [ -f "$INVENTORY" ]; then
    FEATURES="$(awk -F'\t' -v d="$DEVICE" 'NR > 1 && $1 == d { print $4; exit }' "$INVENTORY")"
fi
[ -n "${FEATURES:-}" ] || fail "FEATURES unset and $DEVICE has no row in $INVENTORY"
case "$FEATURES" in
*chip-esp32c3*) RTARGET=riscv32imc-unknown-none-elf; CHIP=esp32c3 ;;
*)              RTARGET=xtensa-esp32-none-elf;       CHIP=esp32 ;;
esac

STAGING_BUNDLE_NAME="$(basename "$STAGING_BUNDLE")"
set -a
# shellcheck source=/dev/null
. "$REPO_ROOT/.env.dev"
set +a
export HOST="$STAGING_HOST" API_PATH=/ CA_PATH="ca/$STAGING_BUNDLE_NAME" CERT_PATH="certs/" DEVICE_NAME="$DEVICE" FW_VERSION
if [ -f "$HOME/export-esp.sh" ]; then
    # shellcheck source=/dev/null
    . "$HOME/export-esp.sh" >/dev/null 2>&1 || true
fi

BASE="https://$STAGING_HOST"
log "current fleet row for $DEVICE:"
admin_curl -fsS --cacert "$STAGING_BUNDLE" "$BASE/fleet" \
    | python3 -I -c 'import json,sys; d=sys.argv[1]; rows=[r for r in json.load(sys.stdin) if r.get("device_id")==d]; print("  ", json.dumps(rows[0]) if rows else "NOT REGISTERED"); sys.exit(0 if rows else 1)' "$DEVICE" \
    || fail "$DEVICE is not registered on staging"

log "building $DEVICE features=$FEATURES FW_VERSION=$FW_VERSION HOST=$HOST"
cd "$FW_ROOT/supervictor/edge" || exit 1
EXTRA=()
[ "$RTARGET" = riscv32imc-unknown-none-elf ] && EXTRA=(--target "$RTARGET")
cargo build --bin supervictor-embedded --features "$FEATURES" ${EXTRA[@]+"${EXTRA[@]}"}
ELF="$FW_ROOT/target/$RTARGET/debug/supervictor-embedded"
strings "$ELF" | grep -Fx -- "$HOST" >/dev/null || fail "baked HOST not found in $ELF"
strings "$ELF" | grep -Fx -- "$FW_VERSION" >/dev/null || fail "FW_VERSION $FW_VERSION not found in $ELF — did the build pick up the env?"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
IMG="$WORK/$DEVICE-$FW_VERSION.bin"
espflash save-image --chip "$CHIP" "$ELF" "$IMG" >/dev/null
SIZE="$(wc -c < "$IMG" | tr -d ' ')"
SLOT=$((0x1F0000))
[ "$SIZE" -le "$SLOT" ] || fail "image is $SIZE bytes, larger than a $SLOT-byte slot"
SHA="$(shasum -a 256 "$IMG" | cut -d' ' -f1)"
log "image $SIZE bytes ($((SIZE * 100 / SLOT))% of a slot) sha256 $SHA"

cd "$REPO_ROOT" || exit 1
log "uploading to $BASE/devices/$DEVICE/firmware/$FW_VERSION"
MANIFEST="$(admin_curl -fsS --cacert "$STAGING_BUNDLE" -X PUT \
    -H 'Content-Type: application/octet-stream' --data-binary "@$IMG" \
    "$BASE/devices/$DEVICE/firmware/$FW_VERSION")" || fail "upload rejected"
echo "  $MANIFEST"
echo "$MANIFEST" | grep -q "\"sha256\":\"$SHA\"" || fail "server sha256 differs from the local image"

if [ "$ACTIVATE" = 0 ]; then
    log "uploaded, not activated. Activate with:"
    log "  curl --cert $ADMIN_PEM --key $ADMIN_KEY --cacert $STAGING_BUNDLE -X PUT $BASE/devices/$DEVICE/desired_fw -d '{\"desired_fw\":\"$FW_VERSION\"}'"
    exit 0
fi

log "setting desired_fw=$FW_VERSION"
admin_curl -fsS --cacert "$STAGING_BUNDLE" -X PUT -H 'Content-Type: application/json' \
    -d "{\"desired_fw\":\"$FW_VERSION\"}" "$BASE/devices/$DEVICE/desired_fw" >/dev/null || fail "desired_fw rejected"
[ "$WAIT" = 1 ] || { log "target set; the device picks it up on its next uplink"; exit 0; }

log "waiting up to 6 min for $DEVICE to report fw=$FW_VERSION (poll every 10 s)"
for _ in $(seq 1 36); do
    sleep 10
    ROW="$(admin_curl -fsS --cacert "$STAGING_BUNDLE" "$BASE/fleet" \
        | python3 -I -c 'import json,sys; d=sys.argv[1]; rows=[r for r in json.load(sys.stdin) if r.get("device_id")==d]; r=rows[0] if rows else {}; print(r.get("fw","-"), r.get("staleness","-"), r.get("fw_pending"), (r.get("last_uplink") or "-")[11:19])' "$DEVICE")"
    log "  fw=$ROW"
    case "$ROW" in "$FW_VERSION "*) log "$DEVICE is on $FW_VERSION"; exit 0 ;; esac
done
fail "$DEVICE did not report $FW_VERSION in time — serial: espflash monitor --port <port> --chip $CHIP --non-interactive --after no-reset --before no-reset-no-sync"
