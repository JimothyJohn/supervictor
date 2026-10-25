#!/usr/bin/env bash
# deploy-dev-lambda.sh — bring the dev Lambda stack (supervictor-dev) up to this checkout.
#
#   ./scripts/deploy-dev-lambda.sh
#
# The blessed path is the Quickstart: full gate, sam build + deploy
# --config-env dev, then the deployed integration suites. This wraps it with
# a Docker check (sam build uses the image), a warning when HEAD does not
# contain origin/dev, and a post-deploy check of /health plus the function's
# LastModified. Needs a Docker daemon and a terminal; expect 10-20 min.
#
# /fleet and /ui are not routed by the SAM template and the raw execute-api
# URL has no mTLS, so health + LastModified is what can be asserted here.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=deploy-dev
need docker aws git curl
docker info >/dev/null 2>&1 || fail "no Docker daemon reachable"
cd "$REPO_ROOT"
git fetch -q https://github.com/JimothyJohn/supervictor.git '+refs/heads/dev:refs/remotes/origin/dev'
log "HEAD $(git rev-parse --short HEAD) on $(git branch --show-current); origin/dev $(git rev-parse --short origin/dev)"
git merge-base --is-ancestor origin/dev HEAD || log "WARNING: HEAD does not contain origin/dev — you are deploying an older tree"
ENV=dev ./Quickstart staging

log "verifying the dev stack"
URL="$(stack_output supervictor-dev SupervictorApiEndpoint)"
curl -fsS "$URL/health"; echo
echo "  function LastModified: $(aws lambda get-function-configuration --function-name "$(lambda_function dev)" --query LastModified --output text)"
