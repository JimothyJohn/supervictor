#!/usr/bin/env bash
# deploy-staging-build.sh — put THIS CHECKOUT's endpoint on a box (STAGE=staging|prod).
#
#   ./scripts/deploy-staging-build.sh            # build + upload + install + probe
#   STAGE=prod ./scripts/deploy-staging-build.sh # the prod box (a release is the normal prod path)
#
# For builds between releases (a release goes through install-staging.sh).
# Builds a static aarch64 musl binary in an arm64 rust container (into
# target-pi/, which keeps the cargo caches between runs), strips it, uploads
# tarball + sha256 to s3://<bucket>/<stage>/builds/<sha>/, presigns both for
# 30 min, and has the box fetch, verify and install them over SSM with the
# installer left in /opt/supervictor/install (install-endpoint.sh --local).
# Ends by probing /health, /ui/history and /fleet/history with the admin cert.
#
#   BUCKET         S3 bucket (default supervictor)
#   RUST_IMAGE     builder image (default rust:1.91-bookworm)
#   ADMIN          admin certificate for the probes (default $USER)
#
# Needs: docker, aws (SSM + S3 on the bucket), git.
# Never run `supervictor-endpoint --version` on the box: there is no such
# flag, it starts a second server and hangs the SSM script.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=deploy
BUCKET="${BUCKET:-supervictor}"
RUST_IMAGE="${RUST_IMAGE:-rust:1.91-bookworm}"
need docker aws git tar shasum
need_admin
docker info >/dev/null 2>&1 || fail "no Docker daemon reachable"
cd "$REPO_ROOT"
INSTANCE_ID="$(staging_instance)"
SHA="$(git rev-parse --short HEAD)"
[ -z "$(git status --porcelain -- supervictor/endpoint supervictor/common)" ] \
    || log "WARNING: endpoint/common have uncommitted changes; tagging the build $SHA anyway"
TARBALL=supervictor-endpoint-aarch64-unknown-linux-musl.tar.gz
BIN=target-pi/aarch64-unknown-linux-musl/release/supervictor-endpoint

log "building $SHA (musl aarch64, release, --locked) in $RUST_IMAGE"
docker run --rm --platform linux/arm64 -v "$PWD":/app -w /app \
    -e CARGO_TARGET_DIR=/app/target-pi -e CARGO_HOME=/app/target-pi/.cargo-home \
    -e CC_aarch64_unknown_linux_musl=musl-gcc "$RUST_IMAGE" bash -c '
set -euo pipefail
apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends musl-tools >/dev/null
rustup target add aarch64-unknown-linux-musl >/dev/null
cargo build -p supervictor-endpoint --release --locked --target aarch64-unknown-linux-musl 2>&1 | tail -2
strip '"$BIN"'
file '"$BIN"' | grep -q "statically linked"'

DIST="$(mktemp -d)"; trap 'rm -rf "$DIST"' EXIT
cp "$BIN" "$DIST/supervictor-endpoint"
(cd "$DIST" && tar czf "$TARBALL" supervictor-endpoint && shasum -a 256 "$TARBALL" > "$TARBALL.sha256")
KEY="$S3_PREFIX/builds/$SHA/$TARBALL"
aws s3 cp --only-show-errors "$DIST/$TARBALL" "s3://$BUCKET/$KEY"
aws s3 cp --only-show-errors "$DIST/$TARBALL.sha256" "s3://$BUCKET/$KEY.sha256"
URL="$(aws s3 presign "s3://$BUCKET/$KEY" --expires-in 1800)"
URL_SHA="$(aws s3 presign "s3://$BUCKET/$KEY.sha256" --expires-in 1800)"
log "uploaded s3://$BUCKET/$KEY"

ssm_run "$INSTANCE_ID" "supervictor endpoint build $SHA" 300 <<REMOTE_EOF | tail -8
set -euo pipefail
cd /tmp && rm -rf svbuild && mkdir svbuild && cd svbuild
curl -fsSL -o $TARBALL '$URL'
curl -fsSL -o $TARBALL.sha256 '$URL_SHA'
sha256sum -c $TARBALL.sha256
/opt/supervictor/install/scripts/install-endpoint.sh --local /tmp/svbuild/$TARBALL
sleep 2
systemctl is-active supervictor-endpoint
journalctl -u supervictor-endpoint -n 4 --no-pager
cd /tmp && rm -rf svbuild
REMOTE_EOF

log "probing $STAGING_HOST"
for p in /health /ui/history /fleet/history; do
    printf '  %-16s ' "$p"
    admin_curl -m 10 --cacert "$STAGING_BUNDLE" -o /dev/null -w '%{http_code}\n' "https://$STAGING_HOST$p"
done
log "$STAGE is on $SHA — install-staging.sh TAG=<tag> puts a release back on it"
