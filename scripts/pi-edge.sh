#!/usr/bin/env bash
# pi-edge.sh — run the desktop client (simulator-enabled) on a Linux edge box for hours, detached.
#
#   ./scripts/pi-edge.sh [staging|prod] [hours]     # default staging, 12 h
#   ./scripts/pi-edge.sh stop                        # kill it
#
# Gives the dashboard a live device that is not an ESP32. Cross-builds the
# aarch64 client in an arm64 rust container into target-pi/ when the binary
# is missing (BUILD=1 forces a rebuild), copies it plus the CA bundle and the
# device identity to ~/supervictor-edge on the box, and starts it under
# nohup + timeout. Nothing system-wide on the box; log: ~/supervictor-edge/edge.log.
#
# The device cert must exist (./Quickstart certs device <DEVICE>) and the
# device must be registered on the target (POST /devices). If no ssh key of
# yours is authorized on the box, run this from a terminal where you can type
# its password (scp + ssh prompt two or three times).
#
#   PI          user@host (default nick@10.0.0.56)
#   DEVICE      device identity to post as (default pi-edge-01)
#   RUST_IMAGE  builder image for the cross-build (default rust:1.91-bookworm)
set -euo pipefail
PI="${PI:-nick@10.0.0.56}"
DEVICE="${DEVICE:-pi-edge-01}"
RUST_IMAGE="${RUST_IMAGE:-rust:1.91-bookworm}"
REPO_ROOT="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel)"
cd "$REPO_ROOT"
if [ "${1:-}" = stop ]; then
    ssh "$PI" 'pkill -f supervictor-desktop && echo stopped || echo "nothing running"'
    exit 0
fi
TARGET_ENV="${1:-staging}"; HOURS="${2:-12}"
BIN=target-pi/aarch64-unknown-linux-gnu/release/supervictor-desktop
case "$TARGET_ENV" in
staging) HOST=staging.supervictor.advin.io; CA=certs/ca/staging-bundle.pem ;;
prod)    HOST=supervictor.advin.io;         CA=certs/ca/AmazonRootCA1.pem ;;
*) echo "target must be staging or prod"; exit 1 ;;
esac
[ -f "certs/devices/$DEVICE/client.pem" ] || { echo "no device cert for $DEVICE (./Quickstart certs device $DEVICE)"; exit 1; }

if [ ! -x "$BIN" ] || [ "${BUILD:-0}" = 1 ]; then
    command -v docker >/dev/null || { echo "no $BIN and no docker to build it"; exit 1; }
    echo "[pi-edge] cross-building supervictor-desktop for aarch64 in $RUST_IMAGE (into target-pi/)"
    docker run --rm --platform linux/arm64 -v "$PWD":/app -w /app/supervictor/edge \
        -e CARGO_TARGET_DIR=/app/target-pi -e CARGO_HOME=/app/target-pi/.cargo-home \
        "$RUST_IMAGE" cargo build --release --features desktop --bin supervictor-desktop --target aarch64-unknown-linux-gnu
fi
file "$BIN" | grep -q 'ARM aarch64' || { echo "$BIN is not an aarch64 binary"; exit 1; }

ssh "$PI" 'mkdir -p ~/supervictor-edge; pkill -f supervictor-desktop || true'
scp -q "$BIN" "$CA" "certs/devices/$DEVICE/client.pem" "certs/devices/$DEVICE/client.key" "$PI:~/supervictor-edge/"
echo "[pi-edge] starting supervictor-desktop on $PI as $DEVICE -> $HOST for ${HOURS}h (INTERVAL_S=5)"
ssh "$PI" "cd ~/supervictor-edge && chmod 600 client.key && chmod +x supervictor-desktop && \
    HOST=$HOST DEVICE_ID=$DEVICE INTERVAL_S=5 CA_PEM=\$PWD/$(basename "$CA") CLIENT_CERT=\$PWD/client.pem CLIENT_KEY=\$PWD/client.key \
    nohup timeout ${HOURS}h ./supervictor-desktop > edge.log 2>&1 < /dev/null & sleep 8; tail -3 edge.log; pgrep -af supervictor-desktop"
echo "[pi-edge] stop with: ./scripts/pi-edge.sh stop"
case "$TARGET_ENV" in
staging) ./Quickstart fleet "$HOST" | grep -E "^DEVICE|^$DEVICE" ;;
esac
