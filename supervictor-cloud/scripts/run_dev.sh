#!/usr/bin/env bash
# run_dev.sh — Build, start sam local, run unit + integration tests
#
# Usage:
#   ./scripts/run_dev.sh           Build + test (full cycle)
#   ./scripts/run_dev.sh --serve   Build + start sam local (leave running for manual testing)
#
# Requirements:
#   sam       AWS SAM CLI
#   docker    Required by sam local
#   uv        Python package manager

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$ROOT_DIR/.." && pwd)"

# Source port from root .env.dev
if [[ -f "$REPO_ROOT/.env.dev" ]]; then
    source "$REPO_ROOT/.env.dev"
fi

SAM_LOCAL_PORT="${SAM_LOCAL_PORT:-3000}"
SAM_LOCAL_URL="http://localhost:${SAM_LOCAL_PORT}"
SAM_PID_FILE="/tmp/supervictor_sam_local.pid"
SAM_LOG_FILE="/tmp/supervictor_sam_local.log"

SERVE_ONLY=false

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

require_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo "Error: '$1' not found on PATH." >&2
        exit 1
    fi
}

cleanup() {
    if [[ -f "$SAM_PID_FILE" ]]; then
        local pid
        pid=$(cat "$SAM_PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "Stopping sam local (PID $pid)..."
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
        rm -f "$SAM_PID_FILE"
    fi
}

wait_for_sam_local() {
    echo "Waiting for sam local on port ${SAM_LOCAL_PORT}..."
    for i in $(seq 1 120); do
        local http_status
        http_status=$(curl -s -o /dev/null -w "%{http_code}" "${SAM_LOCAL_URL}/hello" 2>/dev/null) || true
        if [[ "$http_status" != "000" ]]; then
            echo "sam local ready (HTTP $http_status)."
            return 0
        fi
        sleep 1
    done
    echo "Error: sam local did not start within 120 seconds." >&2
    echo "Check logs at: $SAM_LOG_FILE" >&2
    exit 1
}

# ---------------------------------------------------------------------------
# Parse args
# ---------------------------------------------------------------------------

while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --serve)
            SERVE_ONLY=true
            ;;
        *)
            echo "Unknown parameter: $1" >&2
            exit 1
            ;;
    esac
    shift
done

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------

require_cmd sam
require_cmd docker
require_cmd uv

if ! docker info &>/dev/null; then
    echo "Error: Docker daemon is not running." >&2
    exit 1
fi

cd "$ROOT_DIR"

# ---------------------------------------------------------------------------
# Step 1: Run unit tests (no infra needed)
# ---------------------------------------------------------------------------

echo "Running unit tests..."
uv run pytest tests/unit/ -v

# ---------------------------------------------------------------------------
# Step 2: Build SAM artifacts
# ---------------------------------------------------------------------------

echo ""
echo "Building SAM..."
sam build

# ---------------------------------------------------------------------------
# Step 3: Start sam local
# ---------------------------------------------------------------------------

trap cleanup EXIT

echo "Starting sam local on port ${SAM_LOCAL_PORT}..."
sam local start-api \
    --port "$SAM_LOCAL_PORT" \
    --log-file "$SAM_LOG_FILE" \
    &
echo $! > "$SAM_PID_FILE"

wait_for_sam_local

# ---------------------------------------------------------------------------
# Step 4: Serve or test
# ---------------------------------------------------------------------------

if [[ "$SERVE_ONLY" == "true" ]]; then
    echo ""
    echo "sam local running at ${SAM_LOCAL_URL}"
    echo "  GET  ${SAM_LOCAL_URL}/hello"
    echo "  POST ${SAM_LOCAL_URL}/hello  -d '{\"id\":\"test\",\"current\":42}'"
    echo ""
    echo "Press Ctrl+C to stop."
    # Remove trap so cleanup happens on interrupt
    wait
else
    echo ""
    echo "Running integration tests..."
    SAM_LOCAL_URL="$SAM_LOCAL_URL" \
    uv run pytest tests/integration/ -m local -v

    echo ""
    echo "All tests passed."
fi
