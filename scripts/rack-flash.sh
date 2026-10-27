#!/usr/bin/env bash
# rack-flash.sh — guided, one-board-at-a-time USB flashing for a rack of PoE boards.
#
#   ./scripts/rack-flash.sh [staging|dev|prod]        (default staging)
#
# Needs a terminal: it prompts, and espflash wants the serial port.
#
# Why USB at all: the classic ESP32 ROM bootloader only speaks UART, the Olimex
# boards ship with nothing that listens on the network, and the firmware has no
# OTA path yet (TODO.md P2). PoE powers a factory board; it cannot flash it.
# Each board is flashed once over USB, then racked.
#
# Per board the loop does: detect the new serial port -> espflash board-info
# (chip + MAC) -> look the MAC up in rack-inventory.tsv, or ask the board type
# and name it (poe2-rack-NN / poe-iso-rack-NN) -> issue a device cert if missing
# -> register it on the target -> build + flash via scripts/flash.sh -> say how
# to cable it (the POE2 is NOT isolated: USB out before Ethernet in) -> poll the
# target until the board uplinks -> record it in the inventory.
#
#   INVENTORY  board registry, tab-separated (default: certs/rack-inventory.tsv,
#              beside the device identities; gitignored, and a certs/ symlink
#              carries it into worktrees): device_id, board, mac, features,
#              switch_port, target, flashed_at, uplinked_at. MACs are the stable
#              identifier; ports move. flash.sh reads features from it too.
#   WARM=0     skip the cache-warming builds at start.
#   OWNER      owner_id used at registration (default $USER).
#   ADMIN      admin certificate that reads /fleet (default $USER).
#   FW_ROOT    checkout to build from (default: this one).
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=rack

TARGET_ENV="${1:-staging}"
FW_ROOT="${FW_ROOT:-$REPO_ROOT}"
INVENTORY="${INVENTORY:-$REPO_ROOT/certs/rack-inventory.tsv}"
LOG="$REPO_ROOT/.logs/rack-flash.log"
OWNER="${OWNER:-${USER:-owner}}"
GEN_CERTS="$REPO_ROOT/supervictor/endpoint/scripts/gen_certs.sh"
FLASH="$REPO_ROOT/scripts/flash.sh"

say()   { printf '\n\033[1m[rack] %s\033[0m\n' "$*"; }
note()  { printf '[rack] %s\n' "$*"; }
ask()   { local __v; read -r -p "$1 " __v </dev/tty; printf '%s' "$__v"; }
pause() { read -r -p "$1 [Enter] " _ </dev/tty; }
die()   { printf '\033[31m[rack] %s\033[0m\n' "$*" >&2; exit 1; }

case "$TARGET_ENV" in
staging) HOST="$STAGING_HOST";           CA="$STAGING_BUNDLE";                     API_PATH=/ ;;
dev)     HOST=dev.supervictor.advin.io;  CA="$REPO_ROOT/certs/ca/AmazonRootCA1.pem"; API_PATH=/ ;;
prod)    HOST=supervictor.advin.io;      CA="$REPO_ROOT/certs/ca/AmazonRootCA1.pem"; API_PATH=/ ;;
*) die "target must be staging, dev or prod" ;;
esac

# ── preflight ───────────────────────────────────────────────────────────────
say "preflight ($TARGET_ENV → $HOST)"
[ -t 0 ] || die "needs a terminal (prompts); run it in a native shell"
need espflash cargo openssl curl python3 git
[ -f "$HOME/export-esp.sh" ] || die "$HOME/export-esp.sh missing (./Quickstart install)"
[ -d "$FW_ROOT/supervictor/edge" ] || die "no firmware checkout at $FW_ROOT"
grep -q 'board-poe2' "$FW_ROOT/supervictor/edge/Cargo.toml" || die "$FW_ROOT has no board-poe2 feature — is it on dev?"
[ "$FW_ROOT" = "$REPO_ROOT" ] || [ -e "$FW_ROOT/certs" ] || ln -s "$REPO_ROOT/certs" "$FW_ROOT/certs"
[ -f "$REPO_ROOT/certs/ca/ca.key" ] || die "fleet CA key missing at certs/ca/ca.key — cannot issue device certs"
[ -f "$CA" ] || die "CA bundle missing at $CA"
[ -x "$GEN_CERTS" ] || die "gen_certs.sh missing at $GEN_CERTS"
[ -x "$FLASH" ] || die "flash.sh missing at $FLASH"
need_admin
mkdir -p "$(dirname "$LOG")"
note "firmware checkout: $FW_ROOT @ $(git -C "$FW_ROOT" log --oneline -1) [$(git -C "$FW_ROOT" rev-parse --abbrev-ref HEAD)]"
case "$TARGET_ENV" in
staging)
    admin_curl -f -m 15 --cacert "$CA" "https://$HOST/fleet" >/dev/null || die "staging fleet API unreachable with the admin cert"
    note "staging reachable" ;;
*)
    need aws
    LOG_GROUP="/aws/lambda/$(lambda_function "$TARGET_ENV")"
    note "WARNING: $TARGET_ENV runs behind API Gateway; the Lambda sees every device as anonymous and 403s bound boards (TODO.md open question). Registration may work, uplinks will not until the identity path is fixed." ;;
esac
[ -f "$INVENTORY" ] || printf 'device_id\tboard\tmac\tfeatures\tswitch_port\ttarget\tflashed_at\tuplinked_at\n' > "$INVENTORY"

# Warm the build cache for both wired feature sets while boards get unboxed.
# DEVICE_NAME is baked at compile time, so each board still recompiles the edge
# crate, but with warm deps that is about a minute instead of ten.
if [ "${WARM:-1}" = 1 ]; then
    say "warming the build cache (WARM=0 to skip) — unbox the boards meanwhile"
    (
        set -a
        # shellcheck source=/dev/null
        . "$REPO_ROOT/.env.dev"
        set +a
        CA_PATH="ca/$(basename "$CA")"
        export HOST API_PATH CA_PATH CERT_PATH="certs/"
        # shellcheck source=/dev/null
        . "$HOME/export-esp.sh" >/dev/null 2>&1 || true
        cd "$FW_ROOT/supervictor/edge"
        DEVICE_NAME=warm-poe2    cargo build -q --bin supervictor-embedded --features board-poe2,sim
        DEVICE_NAME=warm-poe-iso cargo build -q --bin supervictor-embedded --features ethernet,sim
    ) || die "warm build failed — fix the firmware build before racking boards"
    note "both feature sets build"
fi

# ── helpers ─────────────────────────────────────────────────────────────────
list_ports() { ls /dev/cu.usbserial-* /dev/cu.usbmodem* /dev/ttyUSB* /dev/ttyACM* 2>/dev/null | sort || true; }

wait_for_port() { # prints the new port
    local before after new
    before="$(list_ports)"
    say "plug the NEXT board into USB (POE2 / non-ISO: it must NOT be on the switch at the same time)"
    pause "Then press"
    after="$(list_ports)"
    new="$(comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after") | grep -v '^$' || true)"
    if [ "$(printf '%s\n' "$new" | grep -c .)" -eq 1 ]; then printf '%s' "$new"; return; fi
    if [ -n "$new" ]; then
        note "several new ports appeared:"; printf '%s\n' "$new" | sed 's/^/   /'
        printf '%s' "$(ask 'which one?')"; return
    fi
    note "no new serial port. Present now:"; printf '%s\n' "$after" | sed 's/^/   /'
    note "(CH340 boards enumerate as usbserial-XXXX; if nothing new, try the cable / another hub port)"
    printf '%s' "$(ask 'port to use (blank = retry):')"
}

board_info() { # <port> -> "chip mac" ; retries the CH340 connect timeout
    local port="$1" out i
    for i in 1 2 3; do
        if out="$(espflash board-info --port "$port" 2>&1)"; then
            printf '%s %s' "$(printf '%s\n' "$out" | awk '/Chip type:/{print $3}')" "$(printf '%s\n' "$out" | awk '/MAC address:/{print tolower($3)}')"
            return 0
        fi
        note "board-info failed (attempt $i): $(printf '%s\n' "$out" | tail -1)"; sleep 2
    done
    return 1
}

inv_lookup() { # <mac> -> tsv row or nothing
    awk -F'\t' -v m="$1" 'NR>1 && $3==m' "$INVENTORY"
}
inv_next_name() { # <prefix> -> prefix-NN not used in the inventory nor certs/devices
    local p="$1" n
    for n in $(seq -f '%02g' 1 99); do
        grep -q "^$p-$n	" "$INVENTORY" && continue
        [ -d "$REPO_ROOT/certs/devices/$p-$n" ] && continue
        printf '%s-%s' "$p" "$n"; return
    done
}
inv_upsert() { # device_id board mac features switch_port target flashed_at uplinked_at
    python3 -I - "$INVENTORY" "$@" <<'PY'
import sys
path, row = sys.argv[1], sys.argv[2:]
lines = open(path).read().splitlines()
hdr, body = lines[0], [l for l in lines[1:] if l.strip()]
row = [c if c else '-' for c in row]          # empty cells collapse under IFS=tab on read-back
body = [l for l in body if l.split('\t')[0] != row[0]]
body.append('\t'.join(row))
open(path, 'w').write('\n'.join([hdr] + sorted(body)) + '\n')
PY
}

register() { # <device>
    local d="$1" subject payload status
    subject="$(openssl x509 -in "$REPO_ROOT/certs/devices/$d/client.pem" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
    payload="$(printf '{"device_id":"%s","owner_id":"%s","subject_dn":"%s"}' "$d" "$OWNER" "$subject")"
    status="$(curl -s -m 20 -o /dev/null -w '%{http_code}' --cacert "$CA" \
        --cert "$REPO_ROOT/certs/devices/$d/client.pem" --key "$REPO_ROOT/certs/devices/$d/client.key" \
        -X POST "https://$HOST${API_PATH%/}/devices" -d "$payload")"
    case "$status" in
    201) note "registered $d ($subject)" ;;
    409) note "$d already registered" ;;
    *) die "registration of $d failed: HTTP $status" ;;
    esac
}

poll_uplink() { # <device> <flashed_at> -> 0 when seen
    local d="$1" t="$2" i
    note "waiting up to 3 min for $d to uplink to $TARGET_ENV (newer than $t)"
    for i in $(seq 1 18); do
        sleep 10
        case "$TARGET_ENV" in
        staging)
            if admin_curl -f -m 15 --cacert "$CA" "https://$HOST/fleet" \
                | python3 -I -c 'import json,sys; b,t=sys.argv[1:]; rows=json.load(sys.stdin); rows=rows if isinstance(rows,list) else rows.get("devices",rows); sys.exit(0 if any(r.get("device_id")==b and (r.get("last_uplink") or "")>t for r in rows) else 1)' "$d" "$t"; then
                return 0; fi ;;
        *) aws logs tail "$LOG_GROUP" --since 2m --format short 2>/dev/null | grep -q "$d" && return 0 ;;
        esac
        printf '.'
    done
    echo; return 1
}

# ── the loop ────────────────────────────────────────────────────────────────
: > "$LOG.tmp"; done_count=0
while :; do
    port=""
    while [ -z "$port" ]; do port="$(wait_for_port)"; done
    [ -e "$port" ] || { note "$port does not exist"; continue; }

    say "reading $port"
    info="$(board_info "$port")" || { note "could not talk to the board on $port (bad cable? board held in boot?) — espflash reset --port $port and retry"; continue; }
    chip="${info% *}"; mac="${info#* }"
    note "chip=$chip mac=$mac"
    if [ "$chip" != esp32 ]; then
        note "that is a $chip, not a classic ESP32 — the Olimex PoE boards are all ESP32; a devkit?"
        [ "$(ask 'skip it? [Y/n]')" = n ] || continue
    fi

    # identity: known MAC → reuse; else board type → name
    row="$(inv_lookup "$mac")"
    if [ -n "$row" ]; then
        IFS=$'\t' read -r device board _ features switch_port _ _ _ <<<"$row"
        [ "$switch_port" = - ] && switch_port=""
        note "known board: $device ($board, features=$features, port ${switch_port:-?})"
        [ "$(ask "re-flash $device? [Y/n]")" = n ] && continue
    else
        say "new board (MAC $mac). What does the silkscreen say?"
        cat <<'MENU'
   1) ESP32-POE2      — WROVER-E module, NO galvanic isolation (never USB + Ethernet together)  → board-poe2
   2) ESP32-POE-ISO   — isolated; USB + Ethernet together is fine                                → ethernet
   3) ESP32-POE       — non-ISO original: not verified on this bench (pin map unconfirmed) → stop
   4) something else  — skip this board
MENU
        case "$(ask 'type [1-4]:')" in
        1) board=ESP32-POE2;    features='board-poe2'; prefix=poe2-rack ;;
        2) board=ESP32-POE-ISO; features=ethernet;   prefix=poe-iso-rack
           mod="$(ask 'module label on the metal can — WROOM or WROVER? [wroom/wrover]')"
           case "$mod" in
           [wW][rR][oO][vV]*) note "a WROVER POE-ISO keeps GPIO16/17 for PSRAM like the POE2 → the RMII clock must move to GPIO0; the ethernet feature reads PHY id 0xffffffff on it. Needs its own board feature (TODO.md open question). Skipping."; continue ;;
           esac ;;
        3) note "plain ESP32-POE: pin map not verified against poe_iso.rs — park it and check the schematic. Skipping."; continue ;;
        *) continue ;;
        esac
        suggested="$(inv_next_name "$prefix")"
        device="$(ask "device name [$suggested]:")"; device="${device:-$suggested}"
        [[ "$device" =~ ^[a-z0-9][a-z0-9-]{1,31}$ ]] || { note "name must be lowercase [a-z0-9-], 2-32 chars"; continue; }
        switch_port=""
    fi
    sp="$(ask "switch port it will live on (1-7; 8 is the router uplink) [${switch_port:-?}]:")"; switch_port="${sp:-$switch_port}"

    # cert + registration
    if [ -f "$REPO_ROOT/certs/devices/$device/client.pem" ]; then note "cert exists for $device"
    else say "issuing cert for $device"; (cd "$REPO_ROOT" && "$GEN_CERTS" device "$device"); fi
    register "$device"

    # build + flash (flash.sh bakes HOST/CA/DEVICE_NAME, retries the CH340 timeout)
    say "building + flashing $device (features=$features) on $port"
    flashed_at=""
    if SERIAL_PORT="$port" FEATURES="$features" VERIFY=0 FW_ROOT="$FW_ROOT" "$FLASH" "$device" "$TARGET_ENV" 2>&1 | tee "$LOG.tmp"; then
        flashed_at="$(awk -F= '/^FLASHED_AT=/{print $2}' "$LOG.tmp" | tail -1)"
    else
        note "flash failed for $device — see above. If the board is dark: espflash reset --port $port"
        inv_upsert "$device" "$board" "$mac" "$features" "$switch_port" "$TARGET_ENV" "FLASH-FAILED" ""
        [ "$(ask 'next board? [Y/n]')" = n ] && break || continue
    fi
    cat "$LOG.tmp" >> "$LOG"

    # cabling — the only step that differs by board
    case "$features" in
    board-poe2) say "UNPLUG THE USB CABLE NOW, then plug the board into switch port ${switch_port:-?} (POE2 is not isolated)" ;;
    *)          say "plug the board into switch port ${switch_port:-?} (ISO board: USB may stay attached)" ;;
    esac
    pause "Cabled?"
    uplinked=""
    if poll_uplink "$device" "$flashed_at"; then
        uplinked="$(date -u +%Y-%m-%dT%H:%M:%S)"; say "$device is on $TARGET_ENV ✔"
    else
        note "$device has not uplinked. Check: switch port LED (link), Extend Mode OFF (ports 1-4 drop to 10 Mbps otherwise), PoE LED on the board."
        case "$features" in ethernet) note "ISO board still on USB? peek: espflash monitor --port $port --chip esp32 --non-interactive --after no-reset --before no-reset-no-sync" ;; esac
    fi
    inv_upsert "$device" "$board" "$mac" "$features" "$switch_port" "$TARGET_ENV" "$flashed_at" "$uplinked"
    done_count=$((done_count+1))

    [ "$(ask 'next board? [Y/n]')" = n ] && break
done
rm -f "$LOG.tmp"

say "done: $done_count board(s) this run. Inventory ($INVENTORY):"
column -t -s $'\t' "$INVENTORY"
if [ "$TARGET_ENV" = staging ]; then
    say "fleet rows on staging"
    admin_curl -f --cacert "$CA" "https://$HOST/fleet" \
        | python3 -I -c 'import json,sys; rows=json.load(sys.stdin); rows=rows if isinstance(rows,list) else rows.get("devices",rows); [print(f"{r.get(\"device_id\"):<20} {r.get(\"staleness\"):<8} fw={r.get(\"fw\")} {r.get(\"last_uplink\")}") for r in rows if not str(r.get("device_id")).startswith("sim-")]'
fi
