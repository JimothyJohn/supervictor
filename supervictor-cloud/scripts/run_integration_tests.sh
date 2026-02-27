#!/usr/bin/env bash
# run_integration_tests.sh — Build, start sam local, and run integration tests
#
# Usage:
#   ./scripts/run_integration_tests.sh              Run local (Tier 1) tests
#   API_ENDPOINT=https://... ./scripts/run_integration_tests.sh   Also run remote mTLS tests
#
# Requirements:
#   openssl   For generating test certificates
#   sam       AWS SAM CLI
#   docker    Required by sam local
#   uv        Python package manager

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CERTS_DIR="$ROOT_DIR/certs"
SAM_LOCAL_PORT=3000
SAM_LOCAL_URL="http://localhost:${SAM_LOCAL_PORT}"
SAM_PID_FILE="/tmp/supervictor_sam_local.pid"
SAM_LOG_FILE="/tmp/supervictor_sam_local.log"

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
trap cleanup EXIT

wait_for_sam_local() {
    echo "Waiting for sam local to be ready on port ${SAM_LOCAL_PORT}..."
    # Note: curl -w "%{http_code}" outputs "000" on connection refused — do NOT append
    # || echo "000" or the status becomes "000000" which incorrectly passes the != check.
    for i in $(seq 1 120); do
        local http_status
        # || true prevents set -e from killing the script when curl gets ECONNREFUSED (exit 7)
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
# Pre-flight checks
# ---------------------------------------------------------------------------

require_cmd openssl
require_cmd sam
require_cmd docker
require_cmd uv

if ! docker info &>/dev/null; then
    echo "Error: Docker daemon is not running." >&2
    exit 1
fi

cd "$ROOT_DIR"

# ---------------------------------------------------------------------------
# Step 1: Generate test certificates (idempotent)
# ---------------------------------------------------------------------------

if [[ ! -f "$CERTS_DIR/ca/ca.pem" ]]; then
    echo "Generating test CA..."
    "$SCRIPT_DIR/gen_certs.sh" ca
fi

if [[ ! -f "$CERTS_DIR/devices/test-device/client.pem" ]]; then
    echo "Generating test-device certificate..."
    "$SCRIPT_DIR/gen_certs.sh" device test-device
fi

# ---------------------------------------------------------------------------
# Step 2: Build SAM artifacts
# ---------------------------------------------------------------------------

echo "Building SAM..."
make build

# ---------------------------------------------------------------------------
# Step 3: Start sam local
# ---------------------------------------------------------------------------

echo "Starting sam local on port ${SAM_LOCAL_PORT}..."
sam local start-api \
    --port "$SAM_LOCAL_PORT" \
    --log-file "$SAM_LOG_FILE" \
    &
echo $! > "$SAM_PID_FILE"

wait_for_sam_local

# ---------------------------------------------------------------------------
# Step 4: Run Tier 1 — Local integration tests
# ---------------------------------------------------------------------------

echo ""
echo "Running local integration tests..."
SAM_LOCAL_URL="$SAM_LOCAL_URL" \
TEST_CERT_DIR="$CERTS_DIR" \
uv run pytest tests/integration/ -m local -v

# ---------------------------------------------------------------------------
# Step 5: Run Tier 2 — Remote mTLS tests (optional)
# ---------------------------------------------------------------------------

if [[ -n "${API_ENDPOINT:-}" ]]; then
    echo ""
    echo "Running remote mTLS integration tests against: $API_ENDPOINT"
    API_ENDPOINT="$API_ENDPOINT" \
    TEST_CERT_DIR="$CERTS_DIR" \
    uv run pytest tests/integration/ -m remote -v
else
    echo ""
    echo "Skipping remote mTLS tests (API_ENDPOINT not set)."
    echo "To run: API_ENDPOINT=https://supervictor.advin.io ./scripts/run_integration_tests.sh"
fi

echo ""
echo "Integration tests complete."
