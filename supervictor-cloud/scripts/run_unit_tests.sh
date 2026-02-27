#!/usr/bin/env bash
# run_unit_tests.sh — Run unit tests (no infrastructure required)
#
# Usage:
#   ./scripts/run_unit_tests.sh

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "Running unit tests..."
uv run pytest tests/unit/ -v
