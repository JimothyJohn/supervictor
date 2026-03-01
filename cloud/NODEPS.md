# Remove Superfluous Dependencies — Cloud (Python)

## Context
The cloud project (`cloud/`) has 6 dev dependencies and 1 runtime dependency. 3 dev deps are completely unused. Of the remaining, `requests` can be replaced with stdlib `urllib`, and `pydantic` (the sole runtime dep) can be replaced with `dataclasses` + manual JSON. Each level is independent. Ordered simplest → hardest so you can stop where the tradeoff breaks.

### Current dependency state
- **Runtime** (`hello_world/pyproject.toml`): `pydantic>=2.0`
- **Dev** (`cloud/pyproject.toml`): `pytest`, `pytest-mock`, `moto[s3]`, `boto3`, `pydantic`, `requests`
- **Tests** (`tests/requirements.txt`): `pytest`, `boto3`, `requests`

---

## Level 1: Remove `pytest-mock`, `moto[s3]`, `boto3` (delete lines)
**Risk: None**

All three are declared as dev dependencies but never imported in any `.py` file. Zero usage.

### Changes
- **`cloud/pyproject.toml`**: Remove these 3 lines from `[dependency-groups] dev`:
  - `"pytest-mock>=3.12",`
  - `"moto[s3]>=5.0",`
  - `"boto3>=1.34",`
- **`cloud/tests/requirements.txt`**: Remove `boto3`

### Verification
```
cd cloud && uv run pytest tests/unit/ -v
```

---

## Level 2: Replace `requests` with `urllib.request` (stdlib)
**Risk: Low**

Used in 1 file only: `tests/integration/test_api_gateway.py`. 12 call sites total — 8 `requests.get()`, 3 `requests.post()`, 1 `requests.exceptions.ConnectionError` catch. All usage is simple HTTP with `timeout` and optional `json`/`cert` kwargs. `urllib.request` from stdlib handles all of these.

### Changes
- **`cloud/tests/integration/test_api_gateway.py`**:
  - Remove `import requests`
  - Add `import json`, `import ssl`, `import urllib.request`, `import urllib.error`
  - Add a small `_Response` wrapper class + `_get()` / `_post()` helpers using `urllib.request`
  - Replace all `requests.get/post` calls with `_get/_post`
  - Replace `requests.exceptions.ConnectionError` with `(ConnectionError, urllib.error.URLError)`
- **`cloud/pyproject.toml`**: Remove `"requests>=2.31",` from dev deps
- **`cloud/tests/requirements.txt`**: Remove `requests`

### Verification
```
cd cloud && uv run pytest tests/unit/ -v
```

---

## Level 3: Replace `pydantic` with `dataclasses` + manual validation (~80 LOC)
**Risk: Medium**

Pydantic is the only runtime dependency (shipped inside the Lambda). It's used for 3 model classes, JSON serialization (`.model_dump_json`), input validation (`.model_validate`), and OpenAPI schema generation (`.model_json_schema`).

### Key insight
The models are flat with 2-4 fields each. `json.dumps` + `dataclasses.asdict` replaces serialization. Manual type checking replaces validation. The OpenAPI schemas are static dicts.

### Pydantic API surface being replaced

| Pydantic call | Replacement |
|---|---|
| `class Foo(BaseModel)` | `@dataclass` |
| `.model_dump_json(exclude_none=True)` (2 sites) | `json.dumps({k:v for k,v in asdict(self).items() if v is not None})` |
| `.model_validate(parsed)` (1 site) | Constructor + manual type check |
| `ValidationError` + `.errors()` (1 catch) | Custom `ValidationError` class |
| `.model_json_schema()` (3 sites) | Hand-written schema dicts |

### Changes
- **`cloud/hello_world/app.py`**: Replace BaseModel → dataclass, add `_validate_uplink`, `_dump_json`, custom `ValidationError`, hand-written OpenAPI schemas
- **`cloud/hello_world/pyproject.toml`**: Remove `pydantic>=2.0` from dependencies
- **`cloud/hello_world/requirements.txt`**: Delete or empty (no deps left)
- **`cloud/pyproject.toml`**: Remove `"pydantic>=2.0",` from dev deps
- **`cloud/tests/unit/test_handler.py`**: Adjust validation error assertions
- **`cloud/tests/unit/test_openapi.py`**: Update schema expectations (Pydantic adds `title`, `$defs` etc.)

### Verification
```
cd cloud && uv run pytest tests/unit/ -v
cd cloud && make build
cd cloud && make local
```
