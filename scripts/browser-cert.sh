#!/usr/bin/env bash
# browser-cert.sh — put the dashboard's admin identity in the keychain of the Mac you sit at.
#
# The certificates live on the build host (no GUI session there), so the
# browser and the keychain trust dialog have to happen on the laptop. Run
# THIS on the laptop in a local Terminal (not over ssh), one line:
#
#   scp <host>:github/supervictor/scripts/browser-cert.sh ~/ && STUDIO=<user@host> ~/browser-cert.sh
#
# It copies the admin cert+key and the fleet CA over scp, builds a .p12
# locally with a password you choose now, puts the identity in a dedicated
# `supervictor` keychain on the search list, trusts the fleet CA for SSL
# (macOS asks for your login password in a dialog), checks the identity
# validates, and opens the dashboard. iphone-cert.sh builds on the files it
# leaves in ~/.supervictor.
#
#   STUDIO       user@host that holds the repo (default nick@10.0.0.41)
#   REMOTE_REPO  repo path on that host (default github/supervictor)
#   ADMIN        admin certificate name under certs/admins/ (default $USER)
#   URL          dashboard to open (default https://staging.supervictor.advin.io/ui)
#
# Undo: security delete-keychain ~/Library/Keychains/supervictor.keychain-db; rm -rf ~/.supervictor
# If the browser keeps prompting for the keychain password:
#   security set-key-partition-list -S apple-tool:,apple:,unsigned: -s ~/Library/Keychains/supervictor.keychain-db
set -euo pipefail
STUDIO="${STUDIO:-nick@10.0.0.41}"
REMOTE_REPO="${REMOTE_REPO:-github/supervictor}"
ADMIN="${ADMIN:-${USER:-admin}}"
URL="${URL:-https://staging.supervictor.advin.io/ui}"
DEST="$HOME/.supervictor"; mkdir -p "$DEST"; chmod 0700 "$DEST"
P12="$DEST/admin-browser.p12"; CA="$DEST/ca.pem"; PEM="$DEST/admin.pem"; KEY="$DEST/admin.key"
KC="$HOME/Library/Keychains/supervictor.keychain-db"

echo "[laptop] fetching admin cert+key ($ADMIN) and the fleet CA from $STUDIO"
scp -q "$STUDIO:$REMOTE_REPO/certs/admins/$ADMIN/admin.pem" "$PEM"
scp -q "$STUDIO:$REMOTE_REPO/certs/admins/$ADMIN/admin.key" "$KEY"
scp -q "$STUDIO:$REMOTE_REPO/certs/ca/ca.pem" "$CA"
chmod 0600 "$KEY"
echo "[laptop] CA: $(openssl x509 -in "$CA" -noout -subject)"
echo "[laptop] admin: $(openssl x509 -in "$PEM" -noout -subject)"

read -r -s -p "choose a password (used for the .p12 and the local supervictor keychain): " PW; echo
[ -n "$PW" ] || { echo "empty password"; exit 1; }

echo "[laptop] building $P12"
P12PASS="$PW" openssl pkcs12 -export -inkey "$KEY" -in "$PEM" -certfile "$CA" -name "$ADMIN" -out "$P12" -passout env:P12PASS
chmod 0600 "$P12"

if [ ! -f "$KC" ]; then
    security create-keychain -p "$PW" "$KC"
    security set-keychain-settings "$KC"
fi
security unlock-keychain -p "$PW" "$KC"
CURRENT="$(security list-keychains -d user | tr -d '" ')"
# shellcheck disable=SC2086  # the keychain list is meant to split
echo "$CURRENT" | grep -qx "$KC" || security list-keychains -d user -s $CURRENT "$KC"

have_identity() {
    security find-identity -p ssl-client "$KC" 2>/dev/null | grep -q "$ADMIN"
}
if have_identity; then echo "[laptop] identity already in $KC"; else
    echo "[laptop] importing identity"
    security import "$P12" -k "$KC" -P "$PW" -f pkcs12 \
        -T /Applications/Safari.app -T "/Applications/Google Chrome.app" -T /usr/bin/security
    have_identity || { echo "[laptop] import stored nothing — fallback: open '$P12' in Keychain Access, choose keychain 'supervictor'"; exit 1; }
fi

if ! security dump-trust-settings -d 2>/dev/null | grep -q SupervictorCA; then
    echo "[laptop] trusting SupervictorCA for SSL (user domain) — macOS asks for your login password in a dialog"
    security add-trusted-cert -r trustRoot -p ssl -k "$KC" "$CA"
fi
echo "[laptop] validated identities:"
security find-identity -p ssl-client -v "$KC" | grep -E "$ADMIN|identities found"
security find-identity -p ssl-client -v "$KC" | grep -q ' 0 valid' && { echo "[laptop] still not valid — check Keychain Access > supervictor > SupervictorCA > Trust"; exit 1; }

echo "[laptop] reachability with the cert: $(curl -sS -m 10 --cacert "$CA" --cert "$PEM" --key "$KEY" -o /dev/null -w '%{http_code}' "$URL" 2>&1 | tail -c 40) (want 200)"
echo "[laptop] opening $URL — pick the '$ADMIN' certificate when asked"
open "$URL" || echo "open failed; paste the URL into Safari or Chrome"
