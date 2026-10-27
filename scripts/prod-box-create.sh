#!/usr/bin/env bash
# prod-box-create.sh — create the prod box: stack supervictor-endpoint-prod at supervictor.advin.io.
#
#   TAG=v0.2.0 ./scripts/prod-box-create.sh      # native terminal; CloudFormation create is yours
#
# Needs, in this order, before it runs:
#   - ./scripts/teardown-lambda-stacks.sh done: the
#     supervictor-prod Lambda stack owned the supervictor.advin.io A record and
#     CloudFormation refuses a name another stack holds (checked below)
#   - the release tagged (TODO card cut-v0-2-0: `TAG=v0.2.0 ./scripts/release.sh`): the box's bootstrap
#     installs TAG from the GitHub release at first boot
# Idempotent: refuses if the stack exists (updates go through
# STAGE=prod scripts/staging-stack-update.sh); a ROLLBACK_COMPLETE stack from a
# failed create must be deleted first (STAGE=prod CONFIRM=supervictor-endpoint-prod
# scripts/teardown-staging.sh) and the script says so.
# Verifies: stack outputs, the AMI pin, then polls https://supervictor.advin.io/health
# with the admin cert until the bootstrap has installed the release (~2 min + DNS TTL 60).
set -euo pipefail
cd "$(dirname "$0")/.."
export STAGE=prod
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
TAG="${TAG:?set TAG, e.g. TAG=v0.2.0 (the release the box installs at first boot)}"
need aws gh curl dig
need_admin
log "as $(aws sts get-caller-identity --query Arn --output text)"

# ── preconditions ──────────────────────────────────────────────────────────
if aws cloudformation describe-stacks --stack-name supervictor-prod >/dev/null 2>&1; then
    fail "the Lambda stack supervictor-prod still exists and owns the supervictor.advin.io record — run ./scripts/teardown-lambda-stacks.sh first"
fi
REC="$(aws route53 list-resource-record-sets --hosted-zone-id Z039212425BG1MHVPYWDN \
    --query "ResourceRecordSets[?Name=='supervictor.advin.io.' && Type=='A'] | length(@)" --output text)"
[ "$REC" = 0 ] || fail "an A record for supervictor.advin.io still exists in Route53 — the stack would fail creating DnsRecord. Remove it (or find which stack owns it) first"
gh release view "$TAG" --repo JimothyJohn/supervictor --json assets --jq '.assets[].name' | grep -q 'aarch64-unknown-linux-musl.tar.gz$' \
    || fail "release $TAG has no aarch64 musl tarball — TAG=$TAG ./scripts/release.sh first"
if ST="$(aws cloudformation describe-stacks --stack-name "$STAGING_STACK" --query 'Stacks[0].StackStatus' --output text 2>/dev/null)"; then
    fail "$STAGING_STACK exists ($ST). Update: STAGE=prod ./scripts/staging-stack-update.sh; a failed create: STAGE=prod CONFIRM=$STAGING_STACK ./scripts/teardown-staging.sh"
fi
log "preconditions ok: name free, $TAG released, no prod stack"

# ── create ─────────────────────────────────────────────────────────────────
TAG="$TAG" ./scripts/deploy-staging.sh

# ── verify ─────────────────────────────────────────────────────────────────
EIP="$(stack_output "$STAGING_STACK" PublicIp)"
log "AMI pin: $(aws ssm get-parameter --name "$AMI_PARAM" --query Parameter.Value --output text)"
log "waiting for DNS ($STAGING_HOST -> $EIP) and the bootstrap install (up to 10 min)"
for i in $(seq 1 60); do
    sleep 10
    DNS="$(dig +short "$STAGING_HOST" | tail -1)"
    if [ "$DNS" = "$EIP" ] && CODE="$(admin_curl -m 10 --cacert "$STAGING_BUNDLE" -o /dev/null -w '%{http_code}' "https://$STAGING_HOST/health" 2>/dev/null)" && [ "$CODE" = 200 ]; then
        log "prod answers: /health 200 with the admin cert (fleet-signed server cert for now)"
        break
    fi
    [ "$i" -eq 60 ] && fail "no 200 from https://$STAGING_HOST/health yet (dns=$DNS). Bootstrap log: aws ssm start-session --target $(staging_instance), then sudo tail -f /var/log/bootstrap.log"
    printf '.'
done
echo "  no client cert (must be refused):"
if curl -sS -m 10 --cacert "$STAGING_BUNDLE" -o /dev/null "https://$STAGING_HOST/health" 2>/dev/null; then
    fail "UNEXPECTED: prod accepted a connection without a client certificate"
fi
echo "  refused, as it should be"
echo "  fleet summary:"; admin_curl -fsS --cacert "$STAGING_BUNDLE" "https://$STAGING_HOST/fleet/summary"; echo
log "done — next: ./scripts/prod-letsencrypt.sh"
