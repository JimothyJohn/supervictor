#!/usr/bin/env bash
# prod-letsencrypt.sh — give the prod box a publicly trusted (Let's Encrypt) server certificate.
#
#   ./scripts/prod-letsencrypt.sh      # native terminal; restarts the prod service (yours)
#
# Runs STAGE=prod scripts/staging-letsencrypt.sh: certbot + certbot-dns-route53
# in a venv on the box, DNS-01 for supervictor.advin.io (the instance role's
# AcmeDns01Route53 policy allows exactly that TXT record), deploy hook into
# /etc/supervictor/{server.pem,server.key}, renew timer. The fleet-signed cert
# stays beside it as *.fleet. Boards pin fleet CA + ISRG Root X1, so they
# accept either. Needs 09 done. Idempotent (certbot keeps a valid cert).
# Verifies with the system trust store (no --cacert) and a no-cert refusal.
set -euo pipefail
cd "$(dirname "$0")/.."
export STAGE=prod
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
need aws curl openssl
need_admin
aws cloudformation describe-stacks --stack-name "$STAGING_STACK" >/dev/null 2>&1 || fail "$STAGING_STACK does not exist — run ./scripts/prod-box-create.sh first"

./scripts/staging-letsencrypt.sh

log "verifying from here with the system trust store (no --cacert)"
CODE="$(admin_curl -m 10 -o /dev/null -w '%{http_code}' "https://$STAGING_HOST/fleet/summary")"
[ "$CODE" = 200 ] || fail "/fleet/summary returned $CODE with the admin cert and the system roots"
echo | openssl s_client -connect "$STAGING_HOST:443" -servername "$STAGING_HOST" 2>/dev/null \
    | openssl x509 -noout -issuer -enddate | sed 's/^/  /'
if curl -sS -m 10 -o /dev/null "https://$STAGING_HOST/health" 2>/dev/null; then
    fail "UNEXPECTED: prod accepted a connection without a client certificate"
fi
log "done: publicly trusted, client cert still required — next: ./scripts/boards-to-prod.sh poe-iso-rack-03 (one canary first)"
