#!/usr/bin/env bash
#
# install-endpoint.sh — put the supervictor endpoint on a generic Linux box.
#
# One static binary, one systemd unit, three PEMs, a SQLite file. No Docker,
# no Caddy, no toolchain on the host. Run as root on the target machine:
#
#   curl -fsSL https://raw.githubusercontent.com/JimothyJohn/supervictor/master/scripts/install-endpoint.sh \
#     | sudo bash -s -- v0.1.0
#
#   # or from a tarball already on the box (air-gapped / testing a build):
#   sudo ./install-endpoint.sh --local supervictor-endpoint-x86_64-unknown-linux-musl.tar.gz
#
# Then drop the mTLS material in place and start:
#
#   /etc/supervictor/server.pem   server certificate (chain), PEM
#   /etc/supervictor/server.key   server private key, PEM
#   /etc/supervictor/ca.pem       CA that device/admin certificates chain to
#   systemctl start supervictor-endpoint
#
# Re-running with a newer tag upgrades the binary and restarts the service;
# /etc/supervictor/env is never overwritten.
#
set -e
set -o nounset
set -o pipefail

REPO="${REPO:-JimothyJohn/supervictor}"
PREFIX="${PREFIX:-/usr/local}"
ETC="${ETC:-/etc/supervictor}"
UNIT_DIR="${UNIT_DIR:-/etc/systemd/system}"
SERVICE="supervictor-endpoint"

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }

usage() {
    sed -n '2,/^set -e/p' "$0" | sed 's/^# \{0,1\}//' | sed '$d'
    exit "${1:-0}"
}

[ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] && usage 0
[ $# -ge 1 ] || usage 1
[ "$(id -u)" -eq 0 ] || die "run as root (sudo): installs under $PREFIX, $ETC and $UNIT_DIR"
command -v systemctl >/dev/null 2>&1 || die "systemd not found — this installer only knows systemd"

case "$(uname -m)" in
aarch64 | arm64) TARGET="aarch64-unknown-linux-musl" ;;
x86_64 | amd64) TARGET="x86_64-unknown-linux-musl" ;;
*) die "unsupported architecture: $(uname -m) (releases ship aarch64 and x86_64)" ;;
esac
TARBALL="$SERVICE-$TARGET.tar.gz"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ "$1" = "--local" ]; then
    [ -n "${2:-}" ] || usage 1
    [ -f "$2" ] || die "no such file: $2"
    log "using local tarball $2"
    cp "$2" "$WORK/$TARBALL"
    [ -f "$2.sha256" ] && cp "$2.sha256" "$WORK/$TARBALL.sha256"
else
    TAG="$1"
    BASE="https://github.com/$REPO/releases/download/$TAG"
    command -v curl >/dev/null 2>&1 || die "curl not found"
    log "fetching $TARBALL from $BASE"
    curl -fsSL -o "$WORK/$TARBALL" "$BASE/$TARBALL" || die "download failed: $BASE/$TARBALL"
    curl -fsSL -o "$WORK/$TARBALL.sha256" "$BASE/$TARBALL.sha256" || die "download failed: $BASE/$TARBALL.sha256"
fi

if [ -f "$WORK/$TARBALL.sha256" ]; then
    log "verifying sha256"
    (cd "$WORK" && sha256sum -c "$TARBALL.sha256" >/dev/null) || die "checksum mismatch for $TARBALL"
else
    log "WARNING: no .sha256 beside the tarball — integrity not verified"
fi

tar -xzf "$WORK/$TARBALL" -C "$WORK"
[ -f "$WORK/$SERVICE" ] || die "tarball did not contain $SERVICE"

log "installing $PREFIX/bin/$SERVICE"
install -d -m 0755 "$PREFIX/bin"
install -m 0755 "$WORK/$SERVICE" "$PREFIX/bin/$SERVICE.new"
mv -f "$PREFIX/bin/$SERVICE.new" "$PREFIX/bin/$SERVICE"

log "preparing $ETC (root-only; the unit passes the PEMs to the service as credentials)"
install -d -m 0700 "$ETC"
if [ ! -f "$ETC/env" ]; then
    cat >"$ETC/env" <<'ENV'
# supervictor-endpoint runtime config (systemd EnvironmentFile).
# TLS_CERT / TLS_KEY / TLS_CLIENT_CA are NOT set here: the unit loads
# /etc/supervictor/{server.pem,server.key,ca.pem} as credentials.
PORT=443
STORE_BACKEND=sqlite
SQLITE_DB_PATH=/var/lib/supervictor/supervictor.db
LOG_LEVEL=info
# Flip to true once every device is registered with its certificate
# (GET /fleet over an admin cert shows who still lacks a subject_dn).
REQUIRE_REGISTRATION=false
# Days before uplink rows expire (DynamoDB only; SQLite keeps them).
UPLINK_TTL_DAYS=30
# OTA firmware images (docs/OTA.md). Under the unit's StateDirectory so the
# dynamic user can write it; unset → the firmware routes answer 503.
FIRMWARE_DIR=/var/lib/supervictor/firmware
ENV
    chmod 0600 "$ETC/env"
    log "  wrote $ETC/env (edit to taste)"
else
    log "  keeping existing $ETC/env"
fi

UNIT_SRC="$(cd "$(dirname "$0")" && pwd)/../supervictor/endpoint/deploy/$SERVICE.service"
if [ -f "$UNIT_SRC" ]; then
    log "installing unit from checkout: $UNIT_SRC"
    install -m 0644 "$UNIT_SRC" "$UNIT_DIR/$SERVICE.service"
else
    UNIT_URL="https://raw.githubusercontent.com/$REPO/master/supervictor/endpoint/deploy/$SERVICE.service"
    log "installing unit from $UNIT_URL"
    curl -fsSL -o "$WORK/$SERVICE.service" "$UNIT_URL" || die "download failed: $UNIT_URL"
    install -m 0644 "$WORK/$SERVICE.service" "$UNIT_DIR/$SERVICE.service"
fi
systemctl daemon-reload
systemctl enable "$SERVICE" >/dev/null 2>&1 || true

missing=""
for pem in server.pem server.key ca.pem; do
    [ -f "$ETC/$pem" ] || missing="$missing $pem"
done
if [ -n "$missing" ]; then
    log "installed. Not started: missing in $ETC:$missing"
    log "  copy them in (./Quickstart certs server <name> <host> issues a server cert from the fleet CA),"
    log "  then: systemctl start $SERVICE && journalctl -u $SERVICE -f"
else
    chmod 0600 "$ETC/server.key"
    log "(re)starting $SERVICE"
    systemctl restart "$SERVICE"
    sleep 1
    systemctl --no-pager --lines=5 status "$SERVICE" || true
    log "done. Verify from a dev machine: ./Quickstart ping <host> 443"
fi
