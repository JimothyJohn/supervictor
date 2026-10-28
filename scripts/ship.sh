#!/usr/bin/env bash
# ship.sh — get what is on dev running on prod: one command, resumable.
#
#   ./scripts/ship.sh                  # plan: what is done, what runs next (read-only)
#   ./scripts/ship.sh go               # run the remaining steps, asking at each gate
#   TAG=v0.2.0 ./scripts/ship.sh go    # pick the tag (default: reuse the v* tag on
#                                      # master's head, else the next patch after the latest)
#
# Each step looks at live state (git, gh, CloudFormation, the prod endpoint)
# and is skipped when already done, so after a failure fix it and re-run:
#
#   1 dev is in master     checked only — dev → master is Nick's manual merge
#   2 tag                  annotated TAG on origin/master's head (local)
#   3 release              scripts/release.sh: push the tag, wait for the binaries
#   4 prod box             first time: scripts/teardown-lambda-stacks.sh (typed
#                          confirm) then scripts/prod-box-create.sh, which installs
#                          TAG at first boot; afterwards: STAGE=prod
#                          scripts/install-staging.sh (restarts the prod service)
#   5 Let's Encrypt        scripts/prod-letsencrypt.sh, until the issuer is LE
#   6 boards               scripts/boards-to-prod.sh: a canary (CANARY, default
#                          poe-iso-rack-03), then every wired board not yet fresh on prod
#
#   YES=1     skip the y/N gates (never the typed Lambda teardown confirm)
#   CANARY    first board to move (default poe-iso-rack-03)
#   FW_VERSION, OWNER, INVENTORY   passed through to boards-to-prod.sh
#
# Needs: git, gh, aws, curl, dig, openssl, python3, the admin cert; step 6 also
# the esp toolchain. Native terminal: the stack create/delete and service
# restarts are denied to agent sessions.
set -euo pipefail
cd "$(dirname "$0")/.."
export STAGE=prod
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
MODE="${1:-plan}"
case "$MODE" in plan | go) ;; *) fail "usage: $0 [plan|go]" ;; esac
REPO="${REPO:-JimothyJohn/supervictor}"
CANARY="${CANARY:-poe-iso-rack-03}"
INVENTORY="${INVENTORY:-$REPO_ROOT/certs/rack-inventory.tsv}"
need git gh aws curl dig openssl python3
need_admin
[ -f "$INVENTORY" ] || fail "no board inventory at $INVENTORY — INVENTORY=<tsv>"

confirm() { # <question> — y/N on the terminal; YES=1 answers yes
    [ "${YES:-0}" = 1 ] && return 0
    local a
    read -r -p "[ship] $1 [y/N] " a </dev/tty
    case "$a" in y | Y | yes) return 0 ;; *) return 1 ;; esac
}
row() { printf '  %-16s %-5s %s\n' "$1" "$2" "$3"; }
stack_exists() { aws cloudformation describe-stacks --stack-name "$1" >/dev/null 2>&1; }
released() { # <tag> — the aarch64 tarball the box installs is attached
    gh release view "$1" --repo "$REPO" --json assets --jq '.assets[].name' 2>/dev/null \
        | grep -q 'aarch64-unknown-linux-musl.tar.gz$'
}
le_issued() {
    echo | openssl s_client -connect "$PROD_HOST:443" -servername "$PROD_HOST" 2>/dev/null \
        | openssl x509 -noout -issuer 2>/dev/null | grep -q "Let's Encrypt"
}
boards_missing() { # wired boards in the inventory that are not fresh on prod, canary first
    local fleet
    fleet="$(admin_curl -fsS -m 15 --cacert "$STAGING_BUNDLE" "https://$PROD_HOST/fleet" 2>/dev/null || echo '[]')"
    printf '%s' "$fleet" | python3 -I -c '
import csv, json, sys
inv, canary = sys.argv[1], sys.argv[2]
fresh = {r.get("device_id") for r in json.load(sys.stdin) if r.get("staleness") == "fresh"}
with open(inv, newline="") as f:
    rows = list(csv.reader(f, delimiter="\t"))[1:]
wired = [r[0] for r in rows if len(r) > 3 and ("ethernet" in r[3] or "board-poe2" in r[3])]
missing = sorted(d for d in wired if d not in fresh)
missing.sort(key=lambda d: d != canary)
print(" ".join(missing))' "$INVENTORY" "$CANARY"
}

git fetch -q origin --tags

# ── 1 dev is in master ─────────────────────────────────────────────────────
BEHIND="$(git rev-list --count origin/master..origin/dev)"
MASTER="$(git rev-parse --short origin/master)"
if [ "$BEHIND" = 0 ]; then
    row "dev in master" "done" "origin/master $MASTER has all of dev"
else
    row "dev in master" STOP "master is $BEHIND commit(s) behind dev — your merge: gh pr create --base master --head dev --title Dev, merge it, re-run"
    [ "$MODE" = plan ] || exit 1
fi

# ── 2 tag ──────────────────────────────────────────────────────────────────
if [ -z "${TAG:-}" ]; then
    TAG="$(git tag --points-at origin/master -l 'v*' --sort=-v:refname | head -1)"
    if [ -z "$TAG" ]; then
        LATEST="$(git tag -l 'v*' --sort=-v:refname | head -1)"
        TAG="$(python3 -I -c 'import sys; t=sys.argv[1].lstrip("v").split(".") if sys.argv[1] else ["0","0","-1"]; t[2]=str(int(t[2])+1); print("v"+".".join(t))' "$LATEST")"
    fi
fi
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "TAG must look like v1.2.3, not $TAG"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    AT="$(git rev-parse --short "$TAG^{commit}")"
    if [ "$AT" = "$MASTER" ]; then
        row "tag" "done" "$TAG -> $AT"
    else
        row "tag" STOP "$TAG already exists at $AT, not master's head $MASTER — pick another: TAG=vX.Y.Z $0 $MODE"
        [ "$MODE" = plan ] || exit 1
    fi
else
    row "tag" next "$TAG on origin/master $MASTER"
fi

# ── 3 release ──────────────────────────────────────────────────────────────
if released "$TAG"; then REL=1; row "release" "done" "$TAG has the musl binaries"; else REL=0; row "release" next "TAG=$TAG ./scripts/release.sh"; fi

# ── 4 prod box ─────────────────────────────────────────────────────────────
if stack_exists "$STAGING_STACK"; then
    BOX=1; row "prod box" next "install $TAG on $STAGING_STACK (restarts the service; the box reports no version, so this always offers)"
else
    BOX=0
    if stack_exists supervictor-prod; then
        row "prod box" next "first time: tear down the Lambda stack supervictor-prod (DynamoDB exported first), then create $STAGING_STACK at $TAG"
    else
        row "prod box" next "first time: create $STAGING_STACK at $TAG"
    fi
fi

# ── 5 + 6 only mean anything once the box answers ──────────────────────────
if [ "$BOX" = 1 ] && le_issued; then row "lets-encrypt" "done" "$PROD_HOST serves a Let's Encrypt certificate"; else row "lets-encrypt" next "./scripts/prod-letsencrypt.sh"; fi
if [ "$BOX" = 1 ]; then
    MISSING="$(boards_missing)"
    if [ -z "$MISSING" ]; then row "boards" "done" "every wired board is fresh on prod"; else row "boards" next "move to prod: $MISSING"; fi
else
    row "boards" next "after the box: canary $CANARY, then the rest of the wired boards"
fi

[ "$MODE" = go ] || { log "plan only — run: TAG=$TAG $0 go"; exit 0; }

# ── go ─────────────────────────────────────────────────────────────────────
if ! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    git tag -a "$TAG" origin/master -m "$TAG"
    log "tagged $TAG -> $MASTER"
fi
[ "$REL" = 1 ] || TAG="$TAG" ./scripts/release.sh
released "$TAG" || fail "release $TAG still has no aarch64 tarball — gh run list --workflow=release.yml"

if [ "$BOX" = 0 ]; then
    if stack_exists supervictor-prod; then
        log "the Lambda stack supervictor-prod still owns $PROD_HOST; teardown deletes it AND supervictor-dev (tables exported to scratch/ first)"
        read -r -p "[ship] type supervictor-prod to tear both down: " a </dev/tty
        [ "$a" = supervictor-prod ] || fail "not confirmed — nothing deleted"
        ./scripts/teardown-lambda-stacks.sh
        ! stack_exists supervictor-prod || fail "supervictor-prod is still there — see the teardown output above"
    fi
    confirm "create $STAGING_STACK at $TAG?" || fail "stopped before the create"
    TAG="$TAG" ./scripts/prod-box-create.sh
else
    confirm "install $TAG on prod ($PROD_HOST) and restart its service?" || fail "stopped before the install"
    TAG="$TAG" ./scripts/install-staging.sh
fi

if ! le_issued; then
    confirm "issue the Let's Encrypt certificate on prod (restarts the service)?" || fail "stopped before Let's Encrypt"
    ./scripts/prod-letsencrypt.sh
fi

MISSING="$(boards_missing)"
if [ -n "$MISSING" ]; then
    read -r -a BOARDS <<<"$MISSING"
    FIRST="${BOARDS[0]}"
    confirm "move $FIRST to prod by OTA first (~3 min)?" || fail "stopped before the boards: $MISSING"
    ./scripts/boards-to-prod.sh "$FIRST"
    if [ "${#BOARDS[@]}" -gt 1 ]; then
        REST=("${BOARDS[@]:1}")
        confirm "$FIRST arrived — move the rest (${REST[*]})?" || fail "stopped after the canary; rest: ${REST[*]}"
        ./scripts/boards-to-prod.sh "${REST[@]}"
    fi
fi
log "done: $TAG on https://$PROD_HOST — dashboard at /ui with your admin.p12"
