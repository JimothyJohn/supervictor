#!/usr/bin/env bash
#
# mcp.sh — launch the supervictor MCP server (what .mcp.json runs).
#
# Builds target/release/supervictor-mcp when it is missing or older than its
# sources, then execs it. stdout is the MCP transport, so everything this
# wrapper says goes to stderr; cargo already keeps stdout clean.
#
# Configuration is the SUPERVICTOR_* environment (supervictor/mcp/src/config.rs):
#   SUPERVICTOR_URL          https://localhost:8443 (the local ./Quickstart endpoint)
#   SUPERVICTOR_CA           certs/ca/ca.pem
#   SUPERVICTOR_ADMIN_CERT   certs/admins/$USER/admin.pem
#   SUPERVICTOR_ADMIN_KEY    certs/admins/$USER/admin.key
# Relative paths resolve against the repo root, where this script cds.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=target/release/supervictor-mcp
if [ ! -x "$BIN" ] || [ -n "$(find supervictor/mcp/src supervictor/common/src -name '*.rs' -newer "$BIN" 2>/dev/null)" ]; then
    echo "[mcp] building $BIN" >&2
    cargo build -q --release -p supervictor-mcp >&2
fi
exec "$BIN"
