# supervictor mcp

Model Context Protocol server (stdio, tools only) that answers fleet and
backend status questions from the endpoint's admin API over mTLS.

## Commands
```
cargo test -p supervictor-mcp                       # unit + the e2e suite (real endpoint, real certs; needs openssl)
cargo clippy -p supervictor-mcp --all-targets -- -D warnings
cargo run -p supervictor-mcp                        # speaks JSON-RPC on stdin/stdout; try: echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run -q -p supervictor-mcp
scripts/mcp.sh                                      # what .mcp.json launches (release build on demand)
```

## Constraints
- No MCP SDK crate: `protocol.rs` speaks the four methods a tools-only
  server needs. Every dependency was already in the workspace lock; adding
  one here is a dependency-review event.
- stdout is the transport. Nothing but JSON-RPC lines goes there; logs go
  to stderr.
- Read-only by design: every tool is a GET, every definition carries
  `readOnlyHint: true`. A mutating tool (threshold, style, revoke) is a
  product decision, not a quick addition.
- Routes come from `supervictor_common::routes`; the endpoint's response
  shapes are consumed as `serde_json::Value` and reshaped in `tools.rs`, so
  a new optional field on the API needs no change here.
- Errors are for an operator reading through a model: name the
  `SUPERVICTOR_*` variable or the `./Quickstart` command that fixes it
  (`client.rs::handshake_error`, `tools.rs::get_json`).
- `tests/e2e.rs` spawns the real binary against the real endpoint with
  certificates from `gen_certs.sh`; keep it that way rather than mocking
  the HTTP layer.
