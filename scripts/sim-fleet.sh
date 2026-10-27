#!/usr/bin/env bash
# Simulated machine fleet: Docker containers that post mTLS telemetry as
# pretend industrial equipment (supervictor/edge/docker-compose.sim.yml).
#
#   ./scripts/sim-fleet.sh up     [staging|prod]   issue certs, register, start
#   ./scripts/sim-fleet.sh down                    stop and remove the containers
#   ./scripts/sim-fleet.sh status [staging|prod]   containers + what the fleet API sees
#   ./scripts/sim-fleet.sh thresholds [staging|prod] re-apply per-profile power thresholds
#   ./scripts/sim-fleet.sh logs   [service]        follow container logs
#
# Env: FLEET_OWNER (owner_id for new registrations, default nick),
#      SIM_SPEED / SIM_INTERVAL_S (passed through to compose),
#      SIM_SERVICES (space-separated subset of services; default all).
# Device certs come from the fleet CA (./Quickstart certs device <name>); the
# endpoint must already trust that CA, which staging and prod do.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$REPO_ROOT/supervictor/edge/docker-compose.sim.yml"
GEN_CERTS="$REPO_ROOT/supervictor/endpoint/scripts/gen_certs.sh"
OWNER="${FLEET_OWNER:-nick}"

log() { printf '[sim-fleet] %s\n' "$*"; }
fail() { log "ERROR: $*" >&2; exit 1; }

compose() { docker compose -f "$COMPOSE_FILE" "$@"; }

services() {
    if [[ -n "${SIM_SERVICES:-}" ]]; then
        printf '%s\n' $SIM_SERVICES
    else
        compose config --services
    fi
}

# Host, CA bundle (repo-root relative) and curl trust flags for a target.
target_env() { # <staging|prod>
    case "$1" in
    staging)
        SIM_HOST=staging.supervictor.advin.io
        SIM_CA=ca/staging-bundle.pem
        ;;
    prod)
        SIM_HOST=supervictor.advin.io
        SIM_CA=ca/AmazonRootCA1.pem
        ;;
    *) fail "target must be staging or prod, got '$1'" ;;
    esac
    export SIM_HOST SIM_CA
    CA_FLAG=(--cacert "$REPO_ROOT/certs/$SIM_CA")
}

ensure_cert() { # <device>
    local dir="$REPO_ROOT/certs/devices/$1"
    if [[ -f "$dir/client.pem" && -f "$dir/client.key" ]]; then
        log "$1: cert present"
    else
        log "$1: issuing device cert"
        [[ -x "$GEN_CERTS" ]] || fail "gen_certs.sh not found at $GEN_CERTS"
        (cd "$REPO_ROOT" && "$GEN_CERTS" device "$1" >/dev/null)
    fi
}

# Per-device power threshold (mA) that makes "on" mean "producing" for the
# profile behind each service: above what the machine draws while powered
# but idle (press pump unloaded, oven fan, compressor controls, ...), below
# its working current. Heated profiles (extruder, molder) still read "on"
# while a heater zone is energised; a single threshold can't separate that.
threshold_for() { # <service> -> mA or empty (fleet default)
    case "$1" in
    sim-extruder-*) echo 15000 ;;   # screw runs ≥ 16.5 A; heaters alone 0/24 A
    sim-press-*) echo 12000 ;;      # idle pump 9 A; approach 13.5 A, bend 45+ A
    sim-cnc-*) echo 10000 ;;        # base 6 A, rapid 8 A; finishing 14+ A
    sim-saw-*) echo 5000 ;;         # blade idling 3.75 A; cutting 9+ A
    sim-oven-*) echo 10000 ;;       # fan 0.8 A; element 25.8 A
    sim-compressor-*) echo 5000 ;;  # controls 0.15 A; pumping 17 A
    sim-conveyor-*) echo 1000 ;;    # controls 0.2 A; empty belt 3.2 A
    sim-molder-*) echo 20000 ;;     # pump idle 12 A; cycle 36-108 A (+heaters)
    sim-welder-*) echo 5000 ;;      # fan 1.5 A; arc 24+ A
    sim-pump-*) echo 5000 ;;        # off or 17+ A
    *) echo "" ;;
    esac
}

# Heated profiles: "on" means producing (decided 2026-10-27), so the screw
# or pump must hold above the threshold this long before the device reads
# on; a heater zone pulsing for seconds never does. The other profiles are
# settled by the single bar.
min_on_for() { # <service> -> seconds or empty (last reading alone)
    case "$1" in
    sim-extruder-* | sim-molder-*) echo 120 ;;
    *) echo "" ;;
    esac
}

register() { # <device>
    local dir="$REPO_ROOT/certs/devices/$1" subject payload status
    subject="$(openssl x509 -in "$dir/client.pem" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
    local threshold min_on
    threshold="$(threshold_for "$1")"
    min_on="$(min_on_for "$1")"
    payload="$(printf '{"device_id":"%s","owner_id":"%s","subject_dn":"%s"%s%s}' \
        "$1" "$OWNER" "$subject" "${threshold:+,\"power_threshold_ma\":$threshold}" "${min_on:+,\"min_on_s\":$min_on}")"
    status="$(curl -sS -o /dev/null -w '%{http_code}' "${CA_FLAG[@]}" \
        --cert "$dir/client.pem" --key "$dir/client.key" \
        -X POST "https://$SIM_HOST/devices" -H 'content-type: application/json' -d "$payload")"
    case "$status" in
    201) log "$1: registered (owner $OWNER${threshold:+, threshold $threshold mA}${min_on:+, min on $min_on s})" ;;
    409) log "$1: already registered" ;;
    *) fail "$1: registration returned HTTP $status" ;;
    esac
}

# Set the per-device threshold and minimum on-time on an already-registered
# device (admin cert). The PUT body replaces both parts; `min_on_s` is sent
# only for profiles that have one, so an endpoint older than 2026-10-27
# (no such field, deny_unknown_fields → 422) still accepts the others.
# Idempotent; skipped with a note when no admin cert is available.
apply_threshold() { # <device>
    local threshold min_on admin_dir="$REPO_ROOT/certs/admins/${FLEET_ADMIN:-nick}" status
    threshold="$(threshold_for "$1")"
    min_on="$(min_on_for "$1")"
    [[ -n "$threshold" || -n "$min_on" ]] || return 0
    if [[ ! -f "$admin_dir/admin.pem" ]]; then
        log "$1: no admin cert at $admin_dir; threshold left as is"
        return 0
    fi
    status="$(curl -sS -o /dev/null -w '%{http_code}' "${CA_FLAG[@]}" \
        --cert "$admin_dir/admin.pem" --key "$admin_dir/admin.key" \
        -X PUT "https://$SIM_HOST/devices/$1/threshold" -H 'content-type: application/json' \
        -d "{\"power_threshold_ma\":${threshold:-null}${min_on:+,\"min_on_s\":$min_on}}")"
    case "$status" in
    200) log "$1: power threshold ${threshold:-default} mA${min_on:+, producing after $min_on s}" ;;
    404|405) log "$1: endpoint has no threshold route yet (HTTP $status); deploy the endpoint, then re-run" ;;
    422) [[ -n "$min_on" ]] || fail "$1: threshold update returned HTTP 422"
         log "$1: endpoint predates min_on_s (HTTP 422); threshold left as is — deploy the endpoint, then re-run" ;;
    *) fail "$1: threshold update returned HTTP $status" ;;
    esac
}

cmd_up() { # [target]
    target_env "${1:-staging}"
    command -v docker >/dev/null || fail "docker not found"
    docker info >/dev/null 2>&1 || fail "docker daemon not reachable"
    local svc
    while read -r svc; do
        ensure_cert "$svc"
        register "$svc"
        apply_threshold "$svc"
    done < <(services)
    log "building image and starting the fleet against $SIM_HOST"
    # shellcheck disable=SC2046
    compose up -d --build $(services | tr '\n' ' ')
    sleep 8
    cmd_status "${1:-staging}"
}

cmd_down() {
    compose down --remove-orphans
}

cmd_status() { # [target]
    target_env "${1:-staging}"
    compose ps --format 'table {{.Service}}\t{{.State}}\t{{.Status}}'
    local admin_dir="$REPO_ROOT/certs/admins/${FLEET_ADMIN:-nick}"
    if [[ -f "$admin_dir/admin.pem" ]]; then
        log "fleet view from $SIM_HOST (sim-* devices):"
        curl -fsS "${CA_FLAG[@]}" --cert "$admin_dir/admin.pem" --key "$admin_dir/admin.key" \
            "https://$SIM_HOST/fleet" | python3 -c '
import json, sys
rows = [d for d in json.load(sys.stdin) if d["device_id"].startswith("sim-")]
fmt = "{:<20} {:<8} {:<9} {:>9} {:>9} {:<8} {}"
print(fmt.format("DEVICE", "STATUS", "FRESH", "mA", "ON >= mA", "POWER", "LAST UPLINK"))
for d in sorted(rows, key=lambda d: d["device_id"]):
    thr = d.get("threshold_ma", "-")
    if d.get("threshold_overridden"):
        thr = f"{thr}*"
    print(fmt.format(d["device_id"], d["status"], d["staleness"],
                     d.get("current", "-"), thr, d.get("power", "-"), d.get("last_uplink", "never")))
if any(d.get("threshold_overridden") for d in rows):
    print("* per-device threshold")
'
    else
        log "no admin cert at $admin_dir; skipping the fleet view"
    fi
}

cmd_logs() { # [service]
    compose logs -f --tail=20 "$@"
}

# Re-apply the per-profile power thresholds to registered devices.
cmd_thresholds() { # [target]
    target_env "${1:-staging}"
    local svc
    while read -r svc; do
        apply_threshold "$svc"
    done < <(services)
}

case "${1:-}" in
up) cmd_up "${2:-staging}" ;;
down) cmd_down ;;
status) cmd_status "${2:-staging}" ;;
thresholds) cmd_thresholds "${2:-staging}" ;;
logs) shift; cmd_logs "$@" ;;
*) sed -n '2,16p' "$0"; exit 1 ;;
esac
