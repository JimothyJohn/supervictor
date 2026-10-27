#!/usr/bin/env bash
# staging-require-registration.sh — flip the staging box to strict
# registration (REQUIRE_REGISTRATION=true: only registered, active devices
# bound to the certificate they present may uplink; owner call 2026-10-23,
# the Lambda-era "flip it in template.yaml" card became this).
#
# 1. lists every device on staging that still lacks a subject_dn (those would
#    be rejected afterwards) and refuses unless FORCE=1 or the list is empty;
#    re-register such a device with `./scripts/rack-flash.sh staging` or
#    `./Quickstart onboard`, or revoke it on /ui
# 2. edits /etc/supervictor/env on the box over SSM and restarts the unit
#    (a restart is yours by the rules, hence this script)
# 3. proves a device ping still works and the fleet is still fresh
#
# New boxes get strict mode from scripts/install-endpoint.sh already.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
# shellcheck disable=SC2034  # read by _lib.sh log()
LOG_TAG=strict
need aws curl python3
need_admin
INSTANCE_ID="$(staging_instance)"

log "devices on $STAGING_HOST without a bound certificate:"
unbound="$(admin_curl -f --cacert "$STAGING_BUNDLE" "https://$STAGING_HOST/devices" \
    | python3 -I -c 'import json,sys; rows=json.load(sys.stdin); rows=rows if isinstance(rows,list) else rows.get("devices",rows); print("\n".join(r["device_id"]+" ("+r.get("status","?")+")" for r in rows if not r.get("subject_dn")))')"
if [ -n "$unbound" ]; then
    echo "$unbound" | sed 's/^/  /'
    [ "${FORCE:-0}" = 1 ] || fail "those devices would be rejected in strict mode — re-register or revoke them, or FORCE=1"
else
    echo "  none"
fi

log "flipping REQUIRE_REGISTRATION=true on $INSTANCE_ID and restarting"
ssm_run "$INSTANCE_ID" "supervictor strict registration" 120 <<'REMOTE_EOF'
set -euo pipefail
grep -q '^REQUIRE_REGISTRATION=' /etc/supervictor/env \
    && sed -i 's/^REQUIRE_REGISTRATION=.*/REQUIRE_REGISTRATION=true/' /etc/supervictor/env \
    || echo 'REQUIRE_REGISTRATION=true' >> /etc/supervictor/env
grep '^REQUIRE_REGISTRATION' /etc/supervictor/env
systemctl restart supervictor-endpoint
sleep 2
systemctl is-active supervictor-endpoint
journalctl -u supervictor-endpoint -n 5 --no-pager
REMOTE_EOF

log "verifying"
PING_DEVICE="${PING_DEVICE:-poe-iso-rack-03}"
if [ -f "$REPO_ROOT/certs/devices/$PING_DEVICE/client.pem" ]; then
    PING_CERTS="$REPO_ROOT/certs/devices/$PING_DEVICE" "$REPO_ROOT/Quickstart" ping "$STAGING_HOST" 443
fi
admin_curl -f --cacert "$STAGING_BUNDLE" "https://$STAGING_HOST/fleet/summary" && echo
log "done — watch the fleet stay fresh for one uplink interval: https://$STAGING_HOST/ui"
