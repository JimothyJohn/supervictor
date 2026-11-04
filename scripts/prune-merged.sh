#!/usr/bin/env bash
# prune-merged.sh — remove merged worktrees, merged/superseded branches (local
# and origin) and Lambda-era ignored leftovers. Dry run by default.
#
#   ./scripts/prune-merged.sh          # print the plan, change nothing
#   GO=1 ./scripts/prune-merged.sh     # do it
#
# A worktree goes when it is clean, no process has its cwd inside it, its
# reflog has not moved in 6 h (a live session), its branch has no open PR, and its HEAD is on origin/dev (ancestor, or every
# commit patch-equivalent per `git cherry`). A branch goes when it is merged
# into origin/dev, or is in SUPERSEDED below at exactly the SHA that was
# triaged (new commits since → kept). dev/master/main, branches checked out in
# a remaining worktree and heads of open PRs are never touched.
# Every deleted ref is logged with its SHA to .logs/prune-<utc>.tsv, so any
# of them comes back with `git branch <name> <sha>`.
# Branch deletion is a hand-off: the session classifier refuses it to agents.
# Idempotent: a second run finds nothing to do. bash 3.2 safe.
set -euo pipefail
cd "$(dirname "$0")/.."
GO="${GO:-0}"
KEEP_RE='^(dev|master|main)$'
log()  { printf '[prune] %s\n' "$*"; }
fail() { log "ERROR: $*" >&2; exit 1; }
for t in git gh lsof; do command -v "$t" >/dev/null 2>&1 || fail "missing tool: $t"; done

# Unmerged branches triaged 2026-11-03 as superseded: name sha reason.
SUPERSEDED='
backend/in-binary-mtls 0bba158fe5f5 settings.json drop == dev PR 67; mTLS landed in PR 66
backend/uplink-identity-binding-stacked 9df4a1f4f517 dynamo tests + mutants.yml on dev; TODO edit moot
docs-cleanup 21168851af39 POE-ISO TODO; POE-ISO support shipped
feat/live-scaled-only 43a8cbd20df1 post-merge commits, patch-equivalent on dev (PR 106)
feat/sample-rate-10hz 5ae4721e9cc2 post-merge commit, patch-equivalent on dev (PR 108)
feat/ui-live-levels afb7b3179cb6 merge-only commit after PR 85
fix-esp-alloc 5031e6dd17d3 July esp-alloc 0.10 bump; dev pins 0.9 with esp-hal 1.0
fix/cloud-phase3-python314 8375a50acae7 Python stack retired; patches equivalent on dev
fix/lwa-no-proxy-fallback b6b1e1d44a4a Lambda Web Adapter retired (PR 122)
fleet-table-live 2387f24c7f96 patch-equivalent on dev (PR 32)
wip/recovered-settings-permissions-20261024 ca22dd3fc4fd in-binary mTLS landed (tls.rs); settings.json untracked
'
superseded_sha() { printf '%s\n' "$SUPERSEDED" | awk -v b="$1" '$1==b {print $2}'; }

git fetch -q --prune origin
OPEN_HEADS="$(gh pr list --state open --limit 200 --json headRefName -q '.[].headRefName')"
has_open_pr() { printf '%s\n' "$OPEN_HEADS" | grep -qxF "$1"; }
on_dev() { # <ref>: ancestor of origin/dev, or every commit patch-equivalent
    git merge-base --is-ancestor "$1" origin/dev && return 0
    [ -z "$(git cherry origin/dev "$1" | grep '^+' || true)" ]
}
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p .logs; RECORD=".logs/prune-$STAMP.tsv"
run() { if [ "$GO" = 1 ]; then "$@"; else log "  would: $*"; fi; }
record() { [ "$GO" = 1 ] && printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$RECORD"; return 0; }
MAIN="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
CWDS="$(lsof -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' || true)"

log "1/5 worktrees"
KEPT=""   # branches that stay checked out somewhere
keep_wt() { log "  keep $1: $2"; KEPT="$KEPT $3"; }
for wt in $(git worktree list --porcelain | awk '/^worktree /{print $2}'); do
    [ "$wt" = "$MAIN" ] && continue
    [ -d "$wt" ] || continue
    b="$(git -C "$wt" branch --show-current)"
    if [ -n "$(git -C "$wt" status --porcelain)" ]; then keep_wt "$wt" dirty "$b"; continue; fi
    if printf '%s\n' "$CWDS" | grep -q "^$wt\(/\|$\)"; then keep_wt "$wt" "a process is inside it" "$b"; continue; fi
    gd="$(git -C "$wt" rev-parse --absolute-git-dir)"
    if [ -n "$(find "$gd/logs/HEAD" -mmin -360 2>/dev/null)" ]; then keep_wt "$wt" "active in the last 6 h" "$b"; continue; fi
    if [ -n "$b" ] && ! echo "$b" | grep -qE "$KEEP_RE" && has_open_pr "$b"; then keep_wt "$wt" "$b has an open PR" "$b"; continue; fi
    if ! (cd "$wt" && on_dev HEAD); then keep_wt "$wt" "not on origin/dev" "$b"; continue; fi
    log "  remove $wt [$b]"
    record worktree "$wt" "$(git -C "$wt" rev-parse HEAD)"
    run git worktree remove "$wt"
done
run git worktree prune

log "2/5 main checkout"
cur="$(git -C "$MAIN" branch --show-current)"
if git worktree list --porcelain | grep -qxF "branch refs/heads/dev" && [ "$GO" = 1 ]; then
    log "  leave $MAIN on $cur: dev is checked out elsewhere"; KEPT="$KEPT $cur"
elif [ "$cur" != dev ] && [ -z "$(git -C "$MAIN" status --porcelain)" ] && (cd "$MAIN" && on_dev HEAD); then
    log "  $MAIN is on merged $cur → dev"
    run git -C "$MAIN" switch dev
    run git -C "$MAIN" pull -q --ff-only origin dev
else
    log "  leave $MAIN on ${cur:-detached}"; KEPT="$KEPT $cur"
fi

stays_checked_out() { case " $KEPT " in *" $1 "*) return 0;; esac; return 1; }

log "3/5 local branches"
for b in $(git for-each-ref --format='%(refname:short)' refs/heads); do
    echo "$b" | grep -qE "$KEEP_RE" && continue
    stays_checked_out "$b" && { log "  keep $b: checked out"; continue; }
    has_open_pr "$b" && { log "  keep $b: open PR"; continue; }
    sha="$(git rev-parse --short=12 "$b")"; why=""
    if git merge-base --is-ancestor "$b" origin/dev; then why="merged"
    elif [ -n "$(superseded_sha "$b")" ]; then
        if [ "$(superseded_sha "$b")" = "$sha" ]; then why="superseded"
        else log "  keep $b: moved since triage ($sha)"; continue; fi
    else log "  keep $b: unmerged, not triaged"; continue; fi
    log "  delete $b ($why, $sha)"
    record branch "$b" "$sha"
    run git branch -D "$b"
done

log "4/5 origin branches"
for r in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin); do
    b="${r#origin/}"; [ "$b" = origin ] || [ "$b" = HEAD ] && continue
    echo "$b" | grep -qE "$KEEP_RE" && continue
    has_open_pr "$b" && { log "  keep origin/$b: open PR"; continue; }
    sha="$(git rev-parse --short=12 "$r")"
    if git merge-base --is-ancestor "$r" origin/dev || [ "$(superseded_sha "$b")" = "$sha" ]; then
        log "  delete origin/$b ($sha)"
        record remote "$b" "$sha"
        run git push -q origin --delete "$b"
    else log "  keep origin/$b: unmerged, not triaged"; fi
done

log "5/5 ignored leftovers"
for p in supervictor/endpoint/.aws-sam .pytest_cache .ruff_cache .python-version .claude/handoff-pr85-live-levels.md; do
    [ -e "$MAIN/$p" ] || continue
    log "  remove $MAIN/$p"
    run rm -rf "${MAIN:?}/$p"
done

if [ "$GO" = 1 ]; then
    git fetch -q --prune origin
    log "done: $(git worktree list | wc -l | tr -d ' ') worktrees, $(git for-each-ref refs/heads | wc -l | tr -d ' ') local branches, $(git for-each-ref refs/remotes/origin | wc -l | tr -d ' ') origin refs"
    [ -f "$RECORD" ] && log "deleted refs + SHAs: $RECORD"
else
    log "dry run — GO=1 ./scripts/prune-merged.sh to apply"
fi
