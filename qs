#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cargo build --quiet --manifest-path "$SCRIPT_DIR/cli/Cargo.toml" 2>/dev/null && exec "$SCRIPT_DIR/cli/target/debug/qs" "$@"
