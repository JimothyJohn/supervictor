#!/usr/bin/env bash
# boards-to-prod.sh — move boards from staging to prod by OTA (no USB).
#
#   ./scripts/boards-to-prod.sh poe-iso-rack-03        # one canary first (~3 min)
#   ./scripts/boards-to-prod.sh                        # then every PoE board still on staging
#   FW_VERSION=0.1.9 ./scripts/boards-to-prod.sh ...   # if 0.1.8 is already taken
#
# Per device: register it on prod with its own certificate (POST /devices with
# device_id, owner_id, subject_dn — the cert binding REQUIRE_REGISTRATION=true
# wants), then `VIA=staging scripts/ota-push.sh <device> prod`: the image is
# built with HOST=supervictor.advin.io, uploaded to and activated on STAGING
# (the box the device talks to today), the device pulls it, boots it, and
# confirms on its first PROD uplink. Its staging row goes stale, then dark —
# expected. A failed trial (prod unreachable, device unregistered) rolls the
# device back to the staging image after three boots; nothing is lost.
# Needs 09 + 10 done, the esp toolchain (~/export-esp.sh), the device certs.
# Idempotent: a device already fresh on prod at FW_VERSION is skipped.
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
FW_VERSION="${FW_VERSION:-0.1.8}"      # rack runs 0.1.6; 0.1.7 is an old staging-bound image in the store
OWNER="${OWNER:-${USER:-owner}}"
INVENTORY="${INVENTORY:-$REPO_ROOT/certs/rack-inventory.tsv}"
need curl openssl python3 cargo espflash
need_admin
PROD="https://$PROD_HOST"; STAGING="https://$STAGING_HOST"
admin_curl -fsS -m 10 --cacert "$STAGING_BUNDLE" -o /dev/null "$PROD/health" || fail "prod does not answer with the admin cert — 09/10 first"

if [ $# -gt 0 ]; then DEVICES=("$@"); else
    # every PoE board in the inventory (wired features); the WiFi devkits stay where they are
    # bash 3.2 (macOS /usr/bin/env bash) has no mapfile
    DEVICES=()
    while IFS= read -r d; do DEVICES+=("$d"); done < <(awk -F'\t' 'NR>1 && ($4 ~ /ethernet|board-poe2/) {print $1}' "$INVENTORY")
fi
[ "${#DEVICES[@]}" -gt 0 ] || fail "no devices given and none with wired features in $INVENTORY"
log "moving to prod at FW_VERSION=$FW_VERSION: ${DEVICES[*]}"

fleet_row() { # <base> <device> -> "fw staleness last_uplink" or "- - -"
    admin_curl -fsS -m 15 --cacert "$STAGING_BUNDLE" "$1/fleet" \
        | python3 -I -c 'import json,sys; d=sys.argv[1]; rows=[r for r in json.load(sys.stdin) if r.get("device_id")==d]; r=rows[0] if rows else {}; print(r.get("fw","-"), r.get("staleness","-"), (r.get("last_uplink") or "-")[:19])' "$2"
}
register_prod() { # <device>
    local d="$1" subject payload status
    subject="$(openssl x509 -in "certs/devices/$d/client.pem" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
    payload="$(printf '{"device_id":"%s","owner_id":"%s","subject_dn":"%s"}' "$d" "$OWNER" "$subject")"
    status="$(curl -s -m 20 -o /dev/null -w '%{http_code}' --cacert "$STAGING_BUNDLE" \
        --cert "certs/devices/$d/client.pem" --key "certs/devices/$d/client.key" \
        -X POST "$PROD/devices" -d "$payload")"
    case "$status" in
    201) log "  registered $d on prod ($subject)" ;;
    409) log "  $d already registered on prod" ;;
    *) fail "registration of $d on prod failed: HTTP $status" ;;
    esac
}

FAILED=()
for d in "${DEVICES[@]}"; do
    [ -f "certs/devices/$d/client.pem" ] || { log "no cert for $d — skipping"; FAILED+=("$d"); continue; }
    log "== $d  staging: $(fleet_row "$STAGING" "$d")  prod: $(fleet_row "$PROD" "$d")"
    case "$(fleet_row "$PROD" "$d")" in "$FW_VERSION fresh"*) log "  already on prod at $FW_VERSION — skipping"; continue ;; esac
    register_prod "$d"
    if FW_VERSION="$FW_VERSION" VIA=staging ./scripts/ota-push.sh "$d" prod; then
        log "  $d is on prod: $(fleet_row "$PROD" "$d")"
    else
        log "  $d did NOT arrive on prod — staging row: $(fleet_row "$STAGING" "$d") (a rolled-back board shows fw=0.1.6 fresh there)"
        FAILED+=("$d")
    fi
done

log "prod fleet now:"
FLEET="$(mktemp)"; trap 'rm -f "$FLEET"' EXIT
admin_curl -fsS --cacert "$STAGING_BUNDLE" -o "$FLEET" "$PROD/fleet"
python3 -I - "$FLEET" <<'PY'
import json, sys
for r in json.load(open(sys.argv[1])):
    print(f'  {r.get("device_id"):<18} fw={r.get("fw")} {r.get("staleness")} {(r.get("last_uplink") or "-")[:19]}')
PY
[ "${#FAILED[@]}" -eq 0 ] || fail "not moved: ${FAILED[*]} — rerun with just those names"
log "done — staging rows for these boards go dark within 2 h; that is the move, not a fault"
