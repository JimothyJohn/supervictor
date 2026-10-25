#!/usr/bin/env bash
# Simulated machine fleet: Docker containers that post mTLS telemetry as
# pretend industrial equipment (supervictor/edge/docker-compose.sim.yml).
#
#   ./scripts/sim-fleet.sh up     [staging|prod]   issue certs, register, start
#   ./scripts/sim-fleet.sh down                    stop and remove the containers
#   ./scripts/sim-fleet.sh status [staging|prod]   containers + what the fleet API sees
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

register() { # <device>
    local dir="$REPO_ROOT/certs/devices/$1" subject payload status
    subject="$(openssl x509 -in "$dir/client.pem" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
    payload="$(printf '{"device_id":"%s","owner_id":"%s","subject_dn":"%s"}' "$1" "$OWNER" "$subject")"
    status="$(curl -sS -o /dev/null -w '%{http_code}' "${CA_FLAG[@]}" \
        --cert "$dir/client.pem" --key "$dir/client.key" \
        -X POST "https://$SIM_HOST/devices" -H 'content-type: application/json' -d "$payload")"
    case "$status" in
    201) log "$1: registered (owner $OWNER)" ;;
    409) log "$1: already registered" ;;
    *) fail "$1: registration returned HTTP $status" ;;
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
fmt = "{:<20} {:<8} {:<9} {:>9} {:<8} {}"
print(fmt.format("DEVICE", "STATUS", "FRESH", "mA", "POWER", "LAST UPLINK"))
for d in sorted(rows, key=lambda d: d["device_id"]):
    print(fmt.format(d["device_id"], d["status"], d["staleness"],
                     d.get("current", "-"), d.get("power", "-"), d.get("last_uplink", "never")))
'
    else
        log "no admin cert at $admin_dir; skipping the fleet view"
    fi
}

cmd_logs() { # [service]
    compose logs -f --tail=20 "$@"
}

case "${1:-}" in
up) cmd_up "${2:-staging}" ;;
down) cmd_down ;;
status) cmd_status "${2:-staging}" ;;
logs) shift; cmd_logs "$@" ;;
*) sed -n '2,15p' "$0"; exit 1 ;;
esac
