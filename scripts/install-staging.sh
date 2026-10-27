#!/usr/bin/env bash
# install-staging.sh — put a RELEASE of the single-binary endpoint on the staging box over SSM.
#
#   TAG=v0.2.0 ./scripts/install-staging.sh
#
# What runs on the box (as root, via AWS-RunShellScript):
#   1. stop + disable the compose/Caddy stack a box built before 2026-10-27
#      ran (it owned :443; a no-op on a box the current template bootstrapped)
#   2. /etc/supervictor/{server.pem,server.key,ca.pem} from the PEMs an old
#      stack deploy placed in /opt/supervictor/certs — unless they are already
#      there (the current bootstrap, or a Let's Encrypt cert from
#      staging-letsencrypt.sh, puts them in /etc/supervictor directly)
#   3. fetch install-endpoint.sh + the unit from the tag and run the installer
#      (downloads the aarch64 musl tarball from the GitHub release, verifies
#      the sha256, installs unit + /etc/supervictor/env, restarts the service)
#   4. print status, listener, last journal lines
# Then, from here: a device ping and /fleet/summary with the admin cert, and a
# check that a connection without a client cert is refused.
#
#   TAG          release tag to install (required); run release.sh first
#   PING_DEVICE  device cert used for the mTLS ping (default test-device; skipped if absent)
#   ADMIN        admin certificate (default $USER)
#
# Needs: aws, curl, dig.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=install
TAG="${TAG:?set TAG, e.g. TAG=v0.2.0 (release.sh pushes it)}"
REPO="${REPO:-JimothyJohn/supervictor}"
PING_DEVICE="${PING_DEVICE:-test-device}"
need aws curl dig
need_admin
INSTANCE_ID="$(staging_instance)"

log "installing $TAG on $INSTANCE_ID ($STAGING_HOST)"
ssm_run "$INSTANCE_ID" "supervictor single-binary install $TAG" 600 <<REMOTE_EOF
set -euo pipefail
RAW=https://raw.githubusercontent.com/$REPO/$TAG
echo "== compose/Caddy stack from the bootstrap (owns :443 on a fresh box)"
systemctl disable --now supervictor.service 2>&1 || true
if systemctl is-active --quiet docker; then docker ps -q | xargs -r docker stop >/dev/null 2>&1 || true; fi
systemctl disable --now docker.service docker.socket 2>&1 || true
echo "== PEMs -> /etc/supervictor (kept if present)"
install -d -m 0700 /etc/supervictor
[ -f /etc/supervictor/server.pem ] || [ ! -f /opt/supervictor/certs/server.pem ] || install -m 0644 /opt/supervictor/certs/server.pem /etc/supervictor/server.pem
[ -f /etc/supervictor/server.key ] || [ ! -f /opt/supervictor/certs/server.key ] || install -m 0600 /opt/supervictor/certs/server.key /etc/supervictor/server.key
[ -f /etc/supervictor/ca.pem ]     || [ ! -f /opt/supervictor/certs/ca.pem ]     || install -m 0644 /opt/supervictor/certs/ca.pem     /etc/supervictor/ca.pem
openssl x509 -in /etc/supervictor/server.pem -noout -subject -issuer -enddate
echo "== installer + unit from $TAG"
install -d /opt/supervictor/install/scripts /opt/supervictor/install/supervictor/endpoint/deploy
curl -fsSL -o /opt/supervictor/install/scripts/install-endpoint.sh "\$RAW/scripts/install-endpoint.sh"
curl -fsSL -o /opt/supervictor/install/supervictor/endpoint/deploy/supervictor-endpoint.service "\$RAW/supervictor/endpoint/deploy/supervictor-endpoint.service"
chmod +x /opt/supervictor/install/scripts/install-endpoint.sh
/opt/supervictor/install/scripts/install-endpoint.sh "$TAG"
sleep 2
echo "== result"
systemctl is-active supervictor-endpoint
ss -ltnp | grep ':443 ' || true
journalctl -u supervictor-endpoint -n 20 --no-pager
REMOTE_EOF

log "verifying from here"
echo "  dns: $(dig +short "$STAGING_HOST" | tail -1)"
if [ -f "$REPO_ROOT/certs/devices/$PING_DEVICE/client.pem" ]; then
    echo "  device ping ($PING_DEVICE cert):"
    PING_CERTS="$REPO_ROOT/certs/devices/$PING_DEVICE" "$REPO_ROOT/Quickstart" ping "$STAGING_HOST" 443
else
    echo "  device ping skipped (no cert for $PING_DEVICE; PING_DEVICE=<name>)"
fi
echo "  fleet summary (admin cert):"
admin_curl -f --cacert "$STAGING_BUNDLE" "https://$STAGING_HOST/fleet/summary" && echo
echo "  no client cert (must be refused):"
if curl -sS -m 10 --cacert "$STAGING_BUNDLE" -o /dev/null "https://$STAGING_HOST/health" 2>/dev/null; then
    fail "UNEXPECTED: accepted without a client cert"
else
    echo "  refused, as it should be"
fi
log "done. Dashboard: https://$STAGING_HOST/ui with certs/admins/$ADMIN/admin.p12 in the browser."
