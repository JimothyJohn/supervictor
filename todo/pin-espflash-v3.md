# Pin espflash v3.3.0 explicitly in the repo

## Context
The project uses espflash v3.x but this is implicit — nothing in the repo declares the required version. The installed version is `3.3.0`. There's a planned upgrade to v4 (tracked in `todo/`), so pinning v3 now makes the current requirement explicit and the future upgrade more visible as a diff.

## Changes

### 1. `device/.cargo/config.toml` — add version comment
- File: `device/.cargo/config.toml:2`
- Add a comment on the runner line noting the pinned version:
  ```toml
  runner = "espflash flash --monitor --chip esp32c3 --log-format defmt"  # espflash 3.3.0
  ```

### 2. `cli/src/commands/onboard/preflight.rs` — add version check
- File: `cli/src/commands/onboard/preflight.rs`
- The `require()` function currently just checks tool presence. Add a constant for the expected espflash version and, if feasible, log a warning when `espflash --version` doesn't match `3.3.0`.
- If the `require` function doesn't support version checking, add a separate `check_espflash_version()` helper in `preflight.rs` that runs `espflash --version`, parses output, and warns on mismatch.

### 3. `device/CLAUDE.md` — note the required version
- File: `device/CLAUDE.md:6`
- Update the espflash mention:
  ```
  cargo run --bin supervictor-embedded --features embedded   # Build + flash via espflash 3.3.0
  ```

### 4. `docs/index.html` — update prerequisites table
- File: `docs/index.html` (around the espflash row, ~line 472)
- Add version `3.3.0` to the espflash entry in the prerequisites/tools table.

## Files to modify
1. `device/.cargo/config.toml` — version comment on runner line
2. `cli/src/commands/onboard/preflight.rs` — version constant + check helper
3. `device/CLAUDE.md` — version annotation
4. `docs/index.html` — version in tools table

## Verification
1. `cargo test --lib --target aarch64-apple-darwin` in `device/` — no breakage
2. `cargo test` in `cli/` — preflight tests still pass
3. `espflash --version` matches the pinned `3.3.0`
4. Review `docs/index.html` in browser
