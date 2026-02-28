#!/usr/bin/env bash
# test.sh — End-to-end devcontainer smoke test.
#
# Phases:
#   1. Device detection (initializeCommand simulation)
#   2. Docker image build
#   3. Tool availability  (runs as esp user)
#   4. Quickstart preflight + qs dev --dry-run (runs as root, HOME=/home/esp)
#      Docker-out-of-Docker: host socket is mounted for SAM / docker checks.
#
# Usage: bash .devcontainer/scripts/test.sh [--no-rebuild]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE="supervictor-devcontainer:test"
REBUILD=true

for arg in "$@"; do
  [[ "$arg" == "--no-rebuild" ]] && REBUILD=false
done

# ── ANSI helpers ──────────────────────────────────────────────────────────────
_BOLD="\033[1m"; _CYAN="\033[36m"; _GREEN="\033[32m"; _RED="\033[31m"; _RESET="\033[0m"
step()    { printf "\n${_BOLD}${_CYAN}[%s] => %s${_RESET}\n" "$(date +%T)" "$*"; }
success() { printf "${_GREEN}%s${_RESET}\n" "$*"; }
fail()    { printf "${_RED}FAIL: %s${_RESET}\n" "$*" >&2; exit 1; }

# ── Phase 1: Device detection ─────────────────────────────────────────────────
step "Phase 1: Device detection"
bash "$SCRIPT_DIR/detect-device.sh"
cat "$REPO_ROOT/.devcontainer/.device.env"

# ── Phase 2: Build ────────────────────────────────────────────────────────────
step "Phase 2: Docker image build (esp32c3, this will take several minutes)"
if $REBUILD; then
  docker build \
    --build-arg CONTAINER_USER=esp \
    --build-arg CONTAINER_GROUP=esp \
    --build-arg ESP_BOARD=esp32c3 \
    --progress=plain \
    -t "$IMAGE" \
    -f "$REPO_ROOT/.devcontainer/Dockerfile" \
    "$REPO_ROOT/.devcontainer/"
else
  echo "  --no-rebuild: skipping build, using existing image $IMAGE"
fi
success "Image ready: $IMAGE"

# ── Phase 3: Tool availability (esp user) ────────────────────────────────────
step "Phase 3: Tool availability checks (as esp user)"
docker run --rm \
  --env-file "$REPO_ROOT/.devcontainer/.device.env" \
  -v "$REPO_ROOT:/home/esp/supervictor:ro" \
  "$IMAGE" \
  bash -c '
    set -euo pipefail
    source ~/export-esp.sh 2>/dev/null || true

    check() {
      local name="$1"; shift
      if out=$("$@" 2>&1 | head -1); then
        printf "  %-20s %s\n" "${name}:" "$out"
      else
        printf "  %-20s NOT FOUND\n" "${name}:"
        exit 1
      fi
    }

    echo ""
    check "uv"              uv --version
    check "sam"             sam --version
    check "aws"             aws --version
    check "docker (cli)"    docker --version
    check "cargo"           cargo --version
    check "rustup"          rustup --version
    check "espflash"        espflash --version
    check "cargo-espflash"  cargo-espflash --version
    check "web-flash"       web-flash --version
    check "python3"         python3 --version
    echo ""
    echo "  Rust targets installed:"
    rustup target list --installed | sed "s/^/    /"
  '

# ── Phase 4: Quickstart preflight + qs dev --dry-run (root, DooD) ─────────────
# The macOS Docker socket is owner-only writable (nick:staff, srwxr-xr-x).
# Inside the container the esp user has no write access, so we run as root
# with HOME=/home/esp so all toolchain paths resolve correctly.
step "Phase 4: Quickstart preflight + qs dev --dry-run (Docker-out-of-Docker)"

# Use the canonical path — Docker Desktop handles /var/run/docker.sock as a special mount
# even though it's a symlink on macOS; resolving it breaks the bind-mount
DOCKER_SOCK="/var/run/docker.sock"

# Pass the host daemon's API version so the container's Docker CLI doesn't
# try to negotiate a newer API than the daemon supports
DOCKER_API_VERSION="$(docker version --format '{{.Server.APIVersion}}' 2>/dev/null || echo "1.43")"

docker run --rm \
  --user root \
  -e HOME=/home/esp \
  -e PATH="/home/esp/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  -e DOCKER_API_VERSION="$DOCKER_API_VERSION" \
  --env-file "$REPO_ROOT/.devcontainer/.device.env" \
  --privileged \
  --group-add dialout \
  -v "$REPO_ROOT:/home/esp/supervictor" \
  -v "$DOCKER_SOCK:/var/run/docker.sock" \
  "$IMAGE" \
  bash -c '
    set -euo pipefail
    source /home/esp/export-esp.sh 2>/dev/null || true
    cd /home/esp/supervictor

    echo ""
    echo "  --- docker daemon reachable ---"
    docker info --format "  Server version: {{.ServerVersion}}" \
      || { echo "  Docker daemon NOT reachable (DooD failed)"; exit 1; }

    echo ""
    echo "  --- quickstart preflight (python check) ---"
    python3 - <<'"'"'EOF'"'"'
import sys
sys.path.insert(0, "/home/esp/supervictor")
from quickstart.preflight import check_tools, check_docker_running
from quickstart import runner

missing = check_tools(["uv", "sam", "docker"])
if missing:
    runner.error(f"Missing tools: {missing}")
    sys.exit(1)
else:
    runner.success("  All required tools found on PATH")

docker_ok = check_docker_running()
if docker_ok:
    runner.success("  Docker daemon: reachable")
else:
    runner.error("  Docker daemon: NOT reachable")
    sys.exit(1)
EOF

    echo ""
    echo "  --- qs --dry-run dev ---"
    ./qs --dry-run dev
  '

success "All phases passed."
