#!/usr/bin/env bash
# deploy-prod-lambda.sh — ship master to the prod Lambda stack (supervictor-prod).
#
#   ./scripts/deploy-prod-lambda.sh
#
# Prod ships only via the Quickstart: `./Quickstart prod` = full gate, dev
# stack deploy + deployed tests, a y/N prompt, then sam build/deploy of the
# prod image and the truststore re-pin. It runs from a clean, detached
# checkout of origin/master in a throwaway worktree next to this one so the
# deploy matches what CI tested; .env.* and certs/ are symlinked in. Needs a
# Docker daemon (sam build, PackageType Image) and a terminal for the prompt;
# expect 15-30 min. Afterwards, register any prod board with POST /devices
# against https://supervictor.advin.io before flashing it.
#
#   ADMIN   admin certificate for the post-deploy /fleet/summary check (default $USER)
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=deploy-prod
REMOTE="https://github.com/JimothyJohn/supervictor.git"
PROD_HOST="${PROD_HOST:-supervictor.advin.io}"
need docker aws git curl
need_admin
docker info >/dev/null 2>&1 || fail "no Docker daemon reachable"
cd "$REPO_ROOT"
git fetch -q "$REMOTE" '+refs/heads/master:refs/remotes/origin/master'
WT="$(dirname "$REPO_ROOT")/supervictor-prod-deploy"
[ -d "$WT" ] || git worktree add -q --detach "$WT" origin/master
git -C "$WT" checkout -q --detach origin/master
for f in .env.dev .env.staging .env.prod; do [ -f "$REPO_ROOT/$f" ] && ln -sfn "$REPO_ROOT/$f" "$WT/$f"; done
[ -e "$WT/certs" ] || ln -sfn "$REPO_ROOT/certs" "$WT/certs"
log "deploying $(git -C "$WT" rev-parse --short HEAD) from $WT"
(cd "$WT" && ./Quickstart prod)

log "verifying"
admin_curl -f "https://$PROD_HOST/fleet/summary" && echo
echo "  function LastModified: $(aws lambda get-function-configuration --function-name "$(lambda_function prod)" --query LastModified --output text)"
log "done"
