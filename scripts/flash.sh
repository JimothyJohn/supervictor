#!/usr/bin/env bash
# flash.sh — build + flash one ESP32 board for a target endpoint, then watch it arrive.
#
#   ./scripts/flash.sh <device> <staging|prod> [--monitor]
#
#   device    the DEVICE_NAME baked into the firmware. certs/devices/<device>/
#             must exist (./Quickstart certs device <device>) and the device must
#             be registered on the target (POST /devices; rack-flash.sh does both).
#   staging   HOST=staging.supervictor.advin.io, certs/ca/staging-bundle.pem
#             (fleet CA + ISRG Root X1, so the board survives a server-cert switch)
#   prod      HOST=supervictor.advin.io (PROD_HOST), same bundle — a second box
#             on the single-binary path; nothing answers there until it exists.
#
# Which firmware, which port:
#   FEATURES     board-poe2 | ethernet | wifi,chip-esp32 | wifi,chip-esp32c3.
#                Default: the device's row in certs/rack-inventory.tsv (INVENTORY);
#                otherwise required. `sim` (the telemetry simulator) is added
#                unless SIM=0: the rack has no sensors, so a bench image keeps
#                the simulated traces; SIM=0 is the measured-only production image.
#   SERIAL_PORT  /dev/cu.usb* (macOS) or /dev/tty{USB,ACM}* (Linux). Default:
#                the only serial port present; with several, required. Ports
#                move with hub position; identify a board by its MAC:
#                espflash board-info --port <port>
#                (SERIAL_PORT, not PORT: .env.dev's PORT belongs to the endpoint.)
#   FW_ROOT      checkout to build from (default: this one). Certificates are
#                always read from this repo.
#   VERIFY=0     skip the post-flash arrival poll. A wired board cannot check in
#                until it is cabled; rack-flash.sh polls after the cabling step
#                and reads the FLASHED_AT=<utc> line this prints.
#   ADMIN        admin certificate that reads /fleet on staging (default $USER).
#   --monitor    after the poll, attach espflash monitor for 60 s without
#                resetting the board.
#
# Partition table: supervictor/edge/partitions.csv (OTA A/B layout). espflash
# writes the app to ota_0 and otadata is erased on every USB flash, so the
# bootloader boots what was just written even on a board that had OTA'd into
# ota_1. One USB flash with this layout is what enables OTA on a board.
#
# Needs a terminal for espflash. CH340 boards often time out on connect, so
# three attempts are made. A board left dark after a failed flash:
# espflash reset --port <port>.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=flash
USAGE="usage: flash.sh <device> <staging|prod> [--monitor]"
DEVICE="${1:?$USAGE}"
TARGET_ENV="${2:?$USAGE}"
MONITOR="${3:-}"
FW_ROOT="${FW_ROOT:-$REPO_ROOT}"
INVENTORY="${INVENTORY:-$REPO_ROOT/certs/rack-inventory.tsv}"
need cargo espflash strings
[ -d "$FW_ROOT/supervictor/edge" ] || fail "no firmware checkout at $FW_ROOT"
[ "$FW_ROOT" = "$REPO_ROOT" ] || [ -e "$FW_ROOT/certs" ] || ln -s "$REPO_ROOT/certs" "$FW_ROOT/certs"
[ -f "$REPO_ROOT/certs/devices/$DEVICE/client.pem" ] || fail "no device cert for $DEVICE (./Quickstart certs device $DEVICE)"

if [ -z "${FEATURES:-}" ] && [ -f "$INVENTORY" ]; then
    FEATURES="$(awk -F'\t' -v d="$DEVICE" 'NR > 1 && $1 == d { print $4; exit }' "$INVENTORY")"
fi
[ -n "${FEATURES:-}" ] || fail "FEATURES unset and $DEVICE has no row in $INVENTORY — FEATURES=board-poe2|ethernet|wifi,chip-esp32|wifi,chip-esp32c3"

# Telemetry simulator: the rack has no sensors yet, so bench images keep the
# simulated traces (feature `sim`, off in Cargo by default — a production
# image must never invent a reading). SIM=0 builds the measured-only image.
if [ "${SIM:-1}" != 0 ]; then
    case ",$FEATURES," in *,sim,*) ;; *) FEATURES="$FEATURES,sim" ;; esac
fi
case "$FEATURES" in
*chip-esp32c3*) RTARGET=riscv32imc-unknown-none-elf; CHIP=esp32c3 ;;
*)              RTARGET=xtensa-esp32-none-elf;       CHIP=esp32 ;;
esac

if [ -z "${SERIAL_PORT:-}" ]; then
    ports=()
    for p in /dev/cu.usbserial* /dev/cu.usbmodem* /dev/ttyUSB* /dev/ttyACM*; do [ -e "$p" ] && ports+=("$p"); done
    case ${#ports[@]} in
    0) fail "no serial port present — plug the board in, or set SERIAL_PORT" ;;
    1) SERIAL_PORT="${ports[0]}" ;;
    *) fail "several serial ports (${ports[*]}) — set SERIAL_PORT; identify the board by MAC: espflash board-info --port <port>" ;;
    esac
fi
[ -e "$SERIAL_PORT" ] || fail "port $SERIAL_PORT not present"

STAGING_BUNDLE_NAME="$(basename "$STAGING_BUNDLE")"
set -a
# shellcheck source=/dev/null
. "$REPO_ROOT/.env.dev"
set +a
TARGET_HOST="$(target_host "$TARGET_ENV")"
export HOST="$TARGET_HOST" API_PATH=/ CA_PATH="ca/$STAGING_BUNDLE_NAME"
export CERT_PATH="certs/" DEVICE_NAME="$DEVICE"
[ -f "$REPO_ROOT/$CERT_PATH$CA_PATH" ] || fail "CA bundle missing at $CERT_PATH$CA_PATH"
# Xtensa toolchain env, written by `espup install` (./Quickstart install).
if [ -f "$HOME/export-esp.sh" ]; then
    # shellcheck source=/dev/null
    . "$HOME/export-esp.sh" >/dev/null 2>&1 || true
fi

log "$DEVICE -> $TARGET_ENV  HOST=$HOST API_PATH=$API_PATH CA_PATH=$CA_PATH  features=$FEATURES  port=$SERIAL_PORT"
cd "$FW_ROOT/supervictor/edge"
EXTRA=()
[ "$RTARGET" = riscv32imc-unknown-none-elf ] && EXTRA=(--target "$RTARGET")
cargo build --bin supervictor-embedded --features "$FEATURES" ${EXTRA[@]+"${EXTRA[@]}"}
ELF="$FW_ROOT/target/$RTARGET/debug/supervictor-embedded"
# grep without -q: it must drain stdin, or strings dies of SIGPIPE and pipefail fails the check.
strings "$ELF" | grep -Fx -- "$HOST" >/dev/null || fail "baked HOST not found in $ELF — the build did not pick up the env"

PARTITIONS="$FW_ROOT/supervictor/edge/partitions.csv"
[ -f "$PARTITIONS" ] || fail "partition table missing at $PARTITIONS"
for attempt in 1 2 3; do
    if espflash flash --port "$SERIAL_PORT" --partition-table "$PARTITIONS" --erase-parts otadata "$ELF"; then break; fi
    [ "$attempt" -lt 3 ] && log "espflash failed (CH340 connect timeout is common) — retry $attempt" && sleep 2
    [ "$attempt" -eq 3 ] && fail "gave up; if the board is dark: espflash reset --port $SERIAL_PORT"
done

FLASHED_AT="$(date -u +%Y-%m-%dT%H:%M:%S)"   # ISO-8601 UTC sorts lexically against the fleet's last_uplink
echo "FLASHED_AT=$FLASHED_AT"                  # rack-flash.sh reads this line
if [ "${VERIFY:-1}" = 0 ]; then log "VERIFY=0 — skipping the arrival poll"; exit 0; fi

cd "$REPO_ROOT"
need_admin
log "waiting up to 3 min for $DEVICE to check in at $TARGET_ENV (uplink after $FLASHED_AT)"
for i in $(seq 1 18); do
    sleep 10
    # "fresh" alone is not proof: the pre-flash firmware's last uplink stays fresh for a
    # while. Require an uplink newer than the flash.
    if admin_curl -f --cacert "$STAGING_BUNDLE" "https://$TARGET_HOST/fleet" \
        | python3 -I -c 'import json,sys; b,t=sys.argv[1:]; rows=json.load(sys.stdin); rows=rows if isinstance(rows,list) else rows.get("devices",rows); sys.exit(0 if any(r.get("device_id")==b and (r.get("last_uplink") or "")>t for r in rows) else 1)' "$DEVICE" "$FLASHED_AT"; then
        log "$DEVICE uplinked after the flash — on $TARGET_ENV"; break; fi
    [ "$i" -eq 18 ] && log "not seen yet — attach with: espflash monitor --port $SERIAL_PORT --chip $CHIP --non-interactive --after no-reset --before no-reset-no-sync"
done
if [ "$MONITOR" = "--monitor" ]; then
    perl -e 'alarm 60; exec @ARGV' espflash monitor --port "$SERIAL_PORT" --chip "$CHIP" --non-interactive --after no-reset --before no-reset-no-sync || true
fi
