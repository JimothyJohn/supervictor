#!/usr/bin/env bash
# release.sh — push a v* tag and wait for the release workflow to attach the binaries.
#
#   TAG=v0.2.0 ./scripts/release.sh
#
# The annotated tag must already exist locally (git tag -a v0.2.0 <commit> -m v0.2.0,
# normally on the dev merge commit that master will take). This pushes it over
# HTTPS with gh's credential helper (the ssh agent is usually empty in agent
# sessions), waits for the "Release binaries" workflow run on the tag, and
# confirms the aarch64 musl tarball is attached to the GitHub release. Then:
# TAG=<tag> ./scripts/install-staging.sh. Needs: git, gh.
set -euo pipefail
TAG="${TAG:?set TAG, e.g. TAG=v0.2.0}"
REPO="${REPO:-JimothyJohn/supervictor}"
for t in git gh; do command -v "$t" >/dev/null || { echo "[release] missing tool: $t" >&2; exit 1; }; done

git rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
    || { echo "[release] tag $TAG not found locally; create it: git tag -a $TAG <commit> -m '$TAG'" >&2; exit 1; }
echo "[release] tag $TAG -> $(git rev-parse --short "$TAG^{commit}")"

git -c credential.helper='' -c credential.helper='!gh auth git-credential' \
    push "https://github.com/$REPO.git" "refs/tags/$TAG"

echo "[release] waiting for the Release binaries workflow run on $TAG"
sleep 10
RUN_ID="$(gh run list --repo "$REPO" --workflow=release.yml --limit 5 \
    --json databaseId,headBranch,event --jq "[.[] | select(.headBranch==\"$TAG\")][0].databaseId")"
[ -n "$RUN_ID" ] && [ "$RUN_ID" != "null" ] || { echo "[release] no run found yet for $TAG; check: gh run list --workflow=release.yml" >&2; exit 1; }
gh run watch --repo "$REPO" "$RUN_ID" --exit-status

echo "[release] assets on $TAG:"
gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name'
gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name' \
    | grep -q 'supervictor-endpoint-aarch64-unknown-linux-musl.tar.gz$' \
    || { echo "[release] aarch64 tarball missing from the release" >&2; exit 1; }
echo "[release] done — next: TAG=$TAG ./scripts/install-staging.sh"
