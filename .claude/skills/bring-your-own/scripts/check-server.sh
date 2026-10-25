#!/usr/bin/env bash
#
# check-server.sh — prove a supervictor endpoint from a device's point of view.
#
#   .claude/skills/bring-your-own/scripts/check-server.sh <HOST> [PORT] [DEVICE] [ADMIN]
#
# Runs from the repo root against the certs/ layout `./Quickstart certs`
# writes. Each check prints PASS or FAIL with the reason; the exit status is
# the number of failures. Needs bash, openssl, curl. Portable across macOS,
# Linux and WSL (no GNU-only flags).
#
#   HOST     what the boards dial (DNS name or IP); also the TLS server name
#   PORT     default 443 (boards always use 443; 8443 is the local Quickstart)
#   DEVICE   a device id with a cert in certs/devices/<DEVICE>/ (default: test-device)
#   ADMIN    an admin name with a cert in certs/admins/<ADMIN>/ (default: $USER)
#
# Env: CERTS_DIR (default certs), CA_PEM to verify the SERVER against a
# different CA than the fleet's (e.g. a public root when the server uses
# Let's Encrypt); the device/admin identities still come from the fleet CA.
set -euo pipefail

HOST="${1:?usage: check-server.sh <HOST> [PORT] [DEVICE] [ADMIN]}"
PORT="${2:-443}"
DEVICE="${3:-test-device}"
ADMIN="${4:-${USER:-admin}}"
CERTS_DIR="${CERTS_DIR:-certs}"
CA_PEM="${CA_PEM:-$CERTS_DIR/ca/ca.pem}"
DEV_PEM="$CERTS_DIR/devices/$DEVICE/client.pem"
DEV_KEY="$CERTS_DIR/devices/$DEVICE/client.key"
ADM_PEM="$CERTS_DIR/admins/$ADMIN/admin.pem"
ADM_KEY="$CERTS_DIR/admins/$ADMIN/admin.key"

fails=0
pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*"; fails=$((fails + 1)); }
skip() { printf 'SKIP  %s\n' "$*"; }

for tool in openssl curl; do
    command -v "$tool" >/dev/null 2>&1 || { echo "missing: $tool" >&2; exit 2; }
done
[ -f "$CA_PEM" ] || { echo "no CA at $CA_PEM (./Quickstart certs ca, or set CA_PEM)" >&2; exit 2; }

echo "endpoint https://$HOST:$PORT  device=$DEVICE  admin=$ADMIN  ca=$CA_PEM"

# 1. TCP reachability (bash's /dev/tcp resolves the name too).
if (exec 3<>"/dev/tcp/$HOST/$PORT") 2>/dev/null; then
    pass "tcp: $HOST:$PORT accepts connections"
else
    fail "tcp: cannot connect to $HOST:$PORT — DNS, firewall/security group, or the service is down"
    echo "no point checking further"
    exit "$fails"
fi

# 2. Server certificate: fetch the leaf, check its SAN and expiry.
server_leaf="$(mktemp)"
trap 'rm -f "$server_leaf"' EXIT
if openssl s_client -connect "$HOST:$PORT" -servername "$HOST" -showcerts </dev/null 2>/dev/null \
    | openssl x509 -outform PEM >"$server_leaf" 2>/dev/null && [ -s "$server_leaf" ]; then
    pass "tls: server presented a certificate ($(openssl x509 -in "$server_leaf" -noout -subject | sed 's/^subject=//'))"
else
    fail "tls: no server certificate at $HOST:$PORT — is TLS terminated there (TLS_CERT/TLS_KEY/TLS_CLIENT_CA, or the proxy)?"
    exit "$fails"
fi

# -text rather than -ext: macOS ships LibreSSL, which has no -ext.
san="$(openssl x509 -in "$server_leaf" -noout -text 2>/dev/null \
    | grep -A1 'Subject Alternative Name' | tail -n 1 | tr -d ' ' || true)"
case "$HOST" in
    *[a-zA-Z]*) want="DNS:$HOST" ;;
    *)          want="IP:$HOST" ;;
esac
if printf '%s' "$san" | tr ',' '\n' | grep -qx "$want"; then
    pass "san: server certificate covers $want"
else
    fail "san: server certificate SAN is '${san:-<none>}', does not include $want — reissue: ./Quickstart certs server <name> $HOST (FORCE=1 to overwrite)"
fi

if openssl x509 -in "$server_leaf" -noout -checkend $((30 * 86400)) >/dev/null 2>&1; then
    pass "expiry: server certificate valid for more than 30 days ($(openssl x509 -in "$server_leaf" -noout -enddate | sed 's/^notAfter=//'))"
else
    fail "expiry: server certificate expires within 30 days or already has ($(openssl x509 -in "$server_leaf" -noout -enddate | sed 's/^notAfter=//'))"
fi

if openssl verify -CAfile "$CA_PEM" "$server_leaf" >/dev/null 2>&1; then
    pass "chain: server certificate chains to $CA_PEM (what the boards bake in as CA_PATH)"
else
    fail "chain: server certificate does NOT chain to $CA_PEM — the board's CA_PATH must be the CA that signed it"
fi

# 3. mTLS handshake as the device.
if [ -f "$DEV_PEM" ] && [ -f "$DEV_KEY" ]; then
    out="$(openssl s_client -connect "$HOST:$PORT" -servername "$HOST" \
        -cert "$DEV_PEM" -key "$DEV_KEY" -CAfile "$CA_PEM" </dev/null 2>&1 || true)"
    if printf '%s' "$out" | grep -q "Verify return code: 0 (ok)"; then
        pass "handshake: device $DEVICE completed mTLS"
    else
        fail "handshake: device $DEVICE rejected — $(printf '%s\n' "$out" | grep -E 'Verify return code|alert|error' | head -1) — the endpoint's TLS_CLIENT_CA must be the CA that issued this device cert"
    fi
    code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 --cacert "$CA_PEM" \
        --cert "$DEV_PEM" --key "$DEV_KEY" "https://$HOST:$PORT/health" || true)"
    if [ "$code" = "200" ]; then
        pass "health: GET /health → 200 with the device certificate"
    else
        fail "health: GET /health → $code with the device certificate"
    fi
else
    skip "handshake: no device cert at $DEV_PEM (./Quickstart certs device $DEVICE)"
fi

# 4. Admin access and registration.
if [ -f "$ADM_PEM" ] && [ -f "$ADM_KEY" ]; then
    code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 --cacert "$CA_PEM" \
        --cert "$ADM_PEM" --key "$ADM_KEY" "https://$HOST:$PORT/fleet/summary" || true)"
    case "$code" in
        200) pass "admin: GET /fleet/summary → 200 (OU=admin accepted; behind a proxy this proves the subject header arrives)" ;;
        403) fail "admin: GET /fleet/summary → 403 — the certificate's OU is not admin, or a proxy is not forwarding the subject" ;;
        *)   fail "admin: GET /fleet/summary → $code" ;;
    esac
    if [ -f "$DEV_PEM" ]; then
        # A device certificate must NOT be an admin: a proxy that passes the
        # client's own X-SSL-Client-Subject-DN through would let anyone be.
        code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 --cacert "$CA_PEM" \
            --cert "$DEV_PEM" --key "$DEV_KEY" -H 'X-SSL-Client-Subject-DN: CN=x,OU=admin' \
            "https://$HOST:$PORT/fleet/summary" || true)"
        if [ "$code" = "403" ]; then
            pass "isolation: a device certificate with a forged admin header is refused (403)"
        else
            fail "isolation: a device certificate with a forged admin header got $code — the proxy must strip inbound X-SSL-Client-Subject-DN"
        fi
    fi
    body="$(curl -s --max-time 10 --cacert "$CA_PEM" --cert "$ADM_PEM" --key "$ADM_KEY" \
        "https://$HOST:$PORT/devices" || true)"
    if printf '%s' "$body" | grep -q "\"device_id\":\"$DEVICE\""; then
        if printf '%s' "$body" | tr '{' '\n' | grep "\"device_id\":\"$DEVICE\"" | grep -q '"subject_dn"'; then
            pass "registered: $DEVICE is registered and bound to a certificate subject"
        else
            fail "registered: $DEVICE is registered WITHOUT subject_dn — re-register with the RFC2253 subject (SKILL.md step 3)"
        fi
    else
        fail "registered: $DEVICE is not registered — SKILL.md step 3"
    fi
else
    skip "admin: no admin cert at $ADM_PEM (./Quickstart certs admin $ADMIN)"
fi

echo
if [ "$fails" -eq 0 ]; then
    echo "all checks passed — a board with HOST=$HOST will get the same answers"
else
    echo "$fails check(s) failed"
fi
exit "$fails"
