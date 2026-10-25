#!/usr/bin/env bash
# iphone-cert.sh — put your admin identity on an iPhone (one .p12, no CA step).
#
# The staging server cert comes from Let's Encrypt (staging-letsencrypt.sh),
# so the phone already trusts the server. The only thing to install is YOUR
# client identity, because the dashboard is behind mTLS.
#
# Run on the laptop that ran browser-cert.sh (it holds ~/.supervictor/admin.{pem,key}):
#
#   scp <host>:github/supervictor/scripts/iphone-cert.sh ~/ && ~/iphone-cert.sh
#
# ON THE PHONE:
#   1. AirDrop admin-iphone.p12 -> tap Close on the banner -> Settings > General >
#      VPN & Device Management > "Identity Certificate" > Install (passcode) ->
#      type the .p12 password you choose below. iOS labels it "Not Verified":
#      that is iOS saying the profile isn't signed by an MDM — expected.
#   2. Safari: https://staging.supervictor.advin.io/ui/history -> when asked
#      which certificate to use, pick "<admin> (supervictor admin)" and Allow.
#      Share > Add to Home Screen if you want it as an app.
# Undo on the phone: Settings > General > VPN & Device Management > remove it.
#
#   ADMIN   label on the identity (default $USER)
#   HOST    endpoint whose server cert is checked for public trust (default staging)
set -euo pipefail
ADMIN="${ADMIN:-${USER:-admin}}"
HOST="${HOST:-staging.supervictor.advin.io}"
SRC="$HOME/.supervictor"
OUT="$SRC/iphone"
[ -f "$SRC/admin.pem" ] && [ -f "$SRC/admin.key" ] && [ -f "$SRC/ca.pem" ] || {
    echo "missing $SRC/{admin.pem,admin.key,ca.pem} — run browser-cert.sh first"; exit 1; }
mkdir -p "$OUT"; chmod 0700 "$OUT"
read -r -s -p "choose a password for the iPhone .p12 (you type it once on the phone): " PW; echo
[ -n "$PW" ] || { echo "empty password"; exit 1; }
# LibreSSL's defaults (3DES / RC2-40, SHA-1 MAC) are what iOS's importer
# accepts reliably; the fleet CA rides along as the chain so the profile
# shows who issued the identity.
P12PASS="$PW" openssl pkcs12 -export -inkey "$SRC/admin.key" -in "$SRC/admin.pem" -certfile "$SRC/ca.pem" \
    -name "$ADMIN (supervictor admin)" -out "$OUT/admin-iphone.p12" -passout env:P12PASS
chmod 0600 "$OUT/admin-iphone.p12"
echo "[iphone] verifying the bundle opens with that password"
P12PASS="$PW" openssl pkcs12 -in "$OUT/admin-iphone.p12" -noout -passin env:P12PASS
ls -la "$OUT"
echo "[iphone] checking the server cert is publicly trusted (no --cacert):"
curl -sS -m 10 -o /dev/null -w '  TLS to %{url}: %{ssl_verify_result} (0 = trusted)\n' "https://$HOST/health" || true
echo "[iphone] opening the folder — AirDrop admin-iphone.p12 to the phone; steps in this script's header"
open "$OUT" || true
