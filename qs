#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cargo build --quiet --manifest-path "$SCRIPT_DIR/supervictor/cli/Cargo.toml" 2>/dev/null && exec "$SCRIPT_DIR/target/debug/qs" "$@"
