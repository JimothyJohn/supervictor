# Cloud Library & Python Upgrade Plan

## Current State

| Component | Current | Latest Available |
|---|---|---|
| Python | 3.12 | 3.14 (Lambda supported) |
| Lambda runtime | `python3.12` | `python3.14` |
| pydantic | `>=2.0` (locked 2.12.5) | 2.12.x (current) |
| pytest | `>=8.0` (locked 9.0.2) | 9.x (current) |
| ruff target | `py312` | `py314` |
| CI actions | `@v4` / `@v3` / `@v2` (tags) | Pin to SHAs |

## Issues Found

1. `requests` imported in `tests/integration/test_api_gateway.py` but undeclared in any pyproject.toml or requirements.txt
2. CI actions pinned by tag, not SHA
3. Loose version pins: `pydantic>=2.0` and `pytest>=8.0` are open-ended
4. Python 3.12 is two minor versions behind — 3.13 and 3.14 are Lambda-supported
5. Portal Lambda missing from `template.yaml` (separate issue, not an upgrade concern)

---

## Phase 0: Hygiene (zero risk, no behavior change)

Pin deps and fix undeclared imports before touching anything functional.

- [ ] Add `requests` to `cloud/pyproject.toml` dev dependencies
- [ ] Pin CI action versions to commit SHAs in `python_ci.yml`
  - `actions/checkout@v4` → SHA
  - `astral-sh/setup-uv@v4` → SHA
  - `docker/setup-qemu-action@v3` → SHA
  - `aws-actions/setup-sam@v2` → SHA
- [ ] Tighten version pins: `pydantic>=2.12,<3` and `pytest>=9.0,<10`
- [ ] Run existing tests to confirm baseline passes

**Validate**: `make test` passes, CI green, no functional change.

---

## Phase 1: Update libraries on Python 3.12 (low risk)

Get all deps to latest within current Python. Isolate library changes from runtime changes.

- [ ] `cd cloud && uv lock --upgrade`
- [ ] `cd cloud/uplink && uv lock --upgrade`
- [ ] `cd cloud/portal && uv lock --upgrade`
- [ ] Regenerate requirements via `make deps`
- [ ] `make test` — confirm unit tests pass
- [ ] Run integration tests locally if possible
- [ ] Deploy to dev (`make deploy-dev`) and smoke test

**Validate**: All tests pass, dev deployment healthy, pydantic serialization unchanged.

---

## Phase 2: Python 3.12 → 3.13 (medium risk)

Conservative step — 3.13 is mature and well-supported on Lambda since Nov 2024.

- [ ] Update `requires-python` in all three pyproject.toml files: `>=3.13`
  - `cloud/pyproject.toml`
  - `cloud/uplink/pyproject.toml`
  - `cloud/portal/pyproject.toml`
- [ ] Update `ruff.toml`: `target-version = "py313"`
- [ ] Update `template.yaml`: `Runtime: python3.13`
- [ ] Regenerate all lock files (`uv lock`)
- [ ] Regenerate requirements (`make deps`)
- [ ] Run `uvx ruff check cloud/` — fix any new `UP` (pyupgrade) suggestions
- [ ] `make test`
- [ ] Deploy to dev, smoke test
- [ ] Soak on dev for a cycle before promoting to prod

**Breaking change risks**:
- `typing` module removals — none affect this codebase
- `datetime` changes — portal uses `datetime.now(timezone.utc)` which is fine
- pydantic 2.12+ already supports 3.13

**Validate**: All tests pass, dev deployment healthy, cold start times comparable.

---

## Phase 3: Python 3.13 → 3.14 (higher risk, optional)

Latest runtime. Only after 3.13 is stable in prod.

- [ ] Confirm pydantic formally supports 3.14 (check release notes)
- [ ] Update `requires-python`: `>=3.14`
- [ ] Update `ruff.toml`: `target-version = "py314"`
- [ ] Update `template.yaml`: `Runtime: python3.14`
- [ ] Regenerate locks and requirements
- [ ] Run full test suite
- [ ] Deploy to dev, soak, then prod

**Breaking change risks**:
- 3.14 is newer — wait for pydantic to declare 3.14 support
- Potential stdlib deprecation removals — audit `warnings` output

**Validate**: Same as Phase 2. Also compare cold start latency.

---

## Execution Notes

- Do phases sequentially. Each phase is a separate PR with its own test pass.
- Phase 0 → 1 can be done same day. Phase 2 should soak on dev for at least a week. Phase 3 is optional.
- Portal is zero-risk for all phases (no third-party deps).
- Uplink's only risk surface is pydantic — already on latest 2.x with no deprecated API usage.
- Rollback: every phase is independently revertible by changing the runtime string back and redeploying.
