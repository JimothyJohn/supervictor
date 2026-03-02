# Device Commissioning Deployment Plan

Commission devices by generating certificates, registering them in the cloud, and flashing firmware. Cloud-focused — device changes are high-level only.

## Current Gaps

| Component | Current State | Target State |
|-----------|--------------|--------------|
| Cloud handler | Stateless echo; ignores `client_subject` | Validates device_id against Devices table |
| Cloud infra | No DynamoDB tables | Devices table with GSI on `owner_id` |
| Device firmware | Hardcoded `id: "1234567890"` | Per-device ID from cert CN |
| Cert script | Generates certs, no DB link | Registration step after cert generation |
| CLI | `qs register` stub exists but unwired | Functional `qs register device` calling POST /devices |
| Feature flag | `REQUIRE_DEVICE_REGISTRATION=false` placeholder | Env var read by handler, toggles validation |

---

## End-to-End Commissioning Flow

```
Operator                   CLI (qs)                Cloud                     Device
   │                         │                       │                         │
   ├─ gen_certs.sh device ───┤                       │                         │
   │   <name>                │                       │                         │
   │                         │                       │                         │
   │   Cert files created:   │                       │                         │
   │   certs/devices/<name>/ │                       │                         │
   │    ├── client.pem       │                       │                         │
   │    └── client.key       │                       │                         │
   │                         │                       │                         │
   ├─ qs register device ────┤                       │                         │
   │   --device-id <name>    ├── POST /devices ──────┤                         │
   │   --owner-id <owner>    │   {device_id, owner_id,                        │
   │                         │    subject_dn}         │                         │
   │                         │                       ├── PutItem Devices table  │
   │                         │◄── 201 Created ───────┤                         │
   │                         │                       │                         │
   ├─ qs edge ───────────────┤                       │                         │
   │   (flash with cert +    │                       │                         │
   │    DEVICE_ID env var)   │                       │                         │
   │                         │                       │                         │
   │                         │                       │       Device boots      │
   │                         │                       │◄──── POST / ────────────┤
   │                         │                       │  {id:<name>,current:N}  │
   │                         │                       │                         │
   │                         │                       ├── GetItem Devices table  │
   │                         │                       │   (validate device_id   │
   │                         │                       │    + status=active)     │
   │                         │                       │                         │
   │                         │                       ├──── 200 OK ─────────────┤
```

### Identity Binding

The `subject_dn` stored in the Devices table must match the mTLS `client_subject` extracted by API Gateway:

- **Certificate CN** = device name (set by `gen_certs.sh device <name>`)
- **Devices table `subject_dn`** = `CN=<name>,O=Supervictor,OU=Devices`
- **API Gateway `requestContext.identity.clientCert.subjectDN`** = same string

Cross-checking `client_subject == device.subject_dn` on uplink is a Phase 2 enhancement. For MVP, mTLS guarantees the client holds a CA-signed cert, and the feature flag validates the `device_id` exists in the registry.

---

## Cloud Implementation

### 1. DynamoDB Devices Table

**Data model:**

| Field      | Type   | Notes                        |
|------------|--------|------------------------------|
| device_id  | PK     | Unique device identifier     |
| owner_id   | String | FK to owners, GSI            |
| subject_dn | String | mTLS certificate subject DN  |
| status     | String | active / revoked             |
| created_at | String | ISO 8601                     |

**SAM resource** (add to `cloud/template.yaml`):

```yaml
DevicesTable:
    Type: AWS::DynamoDB::Table
    Properties:
        TableName: !Sub "${AWS::StackName}-devices"
        BillingMode: PAY_PER_REQUEST
        AttributeDefinitions:
            - AttributeName: device_id
              AttributeType: S
            - AttributeName: owner_id
              AttributeType: S
        KeySchema:
            - AttributeName: device_id
              KeyType: HASH
        GlobalSecondaryIndexes:
            - IndexName: owner-index
              KeySchema:
                  - AttributeName: owner_id
                    KeyType: HASH
              Projection:
                  ProjectionType: ALL
        PointInTimeRecoverySpecification:
            PointInTimeRecoveryEnabled: true
        Tags:
            - Key: Environment
              Value: !Ref Environment
```

**Outputs:**

```yaml
DevicesTableName:
    Description: DynamoDB Devices table name
    Value: !Ref DevicesTable

DevicesTableArn:
    Description: DynamoDB Devices table ARN
    Value: !GetAtt DevicesTable.Arn
```

Design rationale:
- `PAY_PER_REQUEST` — scales to zero cost, no capacity planning
- GSI on `owner_id` — needed for "list devices by owner" queries
- Point-in-time recovery — protects against accidental deletes
- Table name includes stack name — isolates dev/prod

### 2. Admin Handler (POST/GET /devices)

Separate Lambda from uplink — different auth (IAM vs mTLS), different IAM scope, different blast radius.

**New file: `cloud/admin/app.py`**

Pydantic models:

```python
class RegisterDeviceRequest(BaseModel):
    device_id: str
    owner_id: str
    subject_dn: str | None = None

    @field_validator("device_id", "owner_id")
    @classmethod
    def must_be_nonempty(cls, v: str) -> str:
        if not v.strip():
            raise ValueError("must not be empty")
        return v.strip()


class DeviceRecord(BaseModel):
    device_id: str
    owner_id: str
    subject_dn: str | None = None
    status: str = "active"
    created_at: str
```

Handler routes:

| Method | Path | Action |
|--------|------|--------|
| POST | /devices | Register device (PutItem with condition to prevent duplicates) |
| GET | /devices | List all devices (Scan, paginated) |
| GET | /devices/{id} | Get single device (GetItem) |

Duplicate registration returns 409 Conflict via `ConditionalCheckFailedException`.

**SAM resource:**

```yaml
AdminFunction:
    Type: AWS::Serverless::Function
    Properties:
        CodeUri: admin/
        Handler: app.lambda_handler
        Runtime: python3.13
        Architectures:
            - arm64
        MemorySize: 256
        Timeout: 10
        Environment:
            Variables:
                ENVIRONMENT: !Ref Environment
                DEVICES_TABLE: !Ref DevicesTable
                LOG_LEVEL: INFO
        Policies:
            - Statement:
                  - Effect: Allow
                    Action:
                        - dynamodb:PutItem
                        - dynamodb:GetItem
                        - dynamodb:Query
                        - dynamodb:Scan
                    Resource:
                        - !GetAtt DevicesTable.Arn
                        - !Sub "${DevicesTable.Arn}/index/*"
            - Statement:
                  - Effect: Allow
                    Action:
                        - logs:CreateLogGroup
                        - logs:CreateLogStream
                        - logs:PutLogEvents
                    Resource: !Sub "arn:aws:logs:${AWS::Region}:${AWS::AccountId}:*"
        Events:
            DevicesPost:
                Type: Api
                Properties:
                    RestApiId: !Ref SupervictorApi
                    Path: /devices
                    Method: post
                    Auth:
                        AuthorizationType: AWS_IAM
            DevicesGet:
                Type: Api
                Properties:
                    RestApiId: !Ref SupervictorApi
                    Path: /devices
                    Method: get
                    Auth:
                        AuthorizationType: AWS_IAM
            DeviceGetById:
                Type: Api
                Properties:
                    RestApiId: !Ref SupervictorApi
                    Path: /devices/{id}
                    Method: get
                    Auth:
                        AuthorizationType: AWS_IAM
```

No `dynamodb:DeleteItem` — devices are soft-revoked, never hard-deleted.

### 3. Uplink Validation (Modify Existing Handler)

Add device validation to `cloud/uplink/app.py` behind a feature flag.

**New SAM parameter:**

```yaml
RequireDeviceRegistration:
    Type: String
    Default: "false"
    AllowedValues:
        - "true"
        - "false"
    Description: When true, uplink rejects messages from unregistered devices.
```

**Add to HelloWorldFunction environment:**

```yaml
DEVICES_TABLE: !Ref DevicesTable
REQUIRE_DEVICE_REGISTRATION: !Ref RequireDeviceRegistration
```

**Add DynamoDB read policy to HelloWorldFunction:**

```yaml
- Statement:
      - Effect: Allow
        Action:
            - dynamodb:GetItem
        Resource: !GetAtt DevicesTable.Arn
```

**Handler logic:**

```python
_REQUIRE_REGISTRATION = os.environ.get(
    "REQUIRE_DEVICE_REGISTRATION", "false"
).lower() == "true"

_devices_table = None

def _get_devices_table():
    global _devices_table
    if _devices_table is None:
        import boto3
        _devices_table = boto3.resource("dynamodb").Table(
            os.environ["DEVICES_TABLE"]
        )
    return _devices_table

def _validate_device(device_id: str) -> tuple[bool, str]:
    if not _REQUIRE_REGISTRATION:
        return True, ""

    table = _get_devices_table()
    resp = table.get_item(Key={"device_id": device_id}, ConsistentRead=True)
    item = resp.get("Item")

    if not item:
        return False, f"Device '{device_id}' not registered"
    if item.get("status") != "active":
        return False, f"Device '{device_id}' status is '{item.get('status')}'"

    return True, ""
```

Call in `_handle_post()` after `UplinkMessage` validation, before response:

```python
is_valid, reason = _validate_device(uplink.id)
if not is_valid:
    return {
        "statusCode": 403,
        "headers": {"Content-Type": "application/json"},
        "body": json.dumps({"error": "Device not authorized", "detail": reason}),
    }
```

Note: `boto3` is pre-installed in Lambda runtime — no need to add it to `uplink/pyproject.toml`. Lazy import avoids cold-start penalty when the flag is off.

### 4. Feature Flag Rollout

| Approach | Pros | Cons |
|----------|------|------|
| **Env var (chosen)** | Zero latency, no extra DB read, per-stack control | Requires redeploy to change |
| DynamoDB flag | Change without redeploy | Extra read on every request |
| SSM Parameter | Change without redeploy, cached | Cache TTL latency, SSM call on cold start |

**Rollout sequence:**

1. Deploy with `RequireDeviceRegistration=false` — all existing devices keep working
2. Register all known devices via `qs register device`
3. Set `RequireDeviceRegistration=true` in `samconfig.toml` parameter overrides
4. Redeploy — unregistered devices now get 403

**samconfig.toml changes:**

```toml
# dev: enabled for testing
[dev.deploy.parameters]
parameter_overrides = "... RequireDeviceRegistration=\"true\""

# prod: disabled initially, flip after registering all devices
[prod.deploy.parameters]
parameter_overrides = "... RequireDeviceRegistration=\"false\""
```

### 5. IAM Policies Summary

| Resource | Action | Function | Purpose |
|----------|--------|----------|---------|
| DevicesTable | `dynamodb:GetItem` | HelloWorldFunction | Validate device on uplink |
| DevicesTable | `dynamodb:PutItem` | AdminFunction | Register new device |
| DevicesTable | `dynamodb:GetItem` | AdminFunction | Look up device |
| DevicesTable | `dynamodb:Query` | AdminFunction | List devices / query by owner |
| DevicesTable/index/* | `dynamodb:Query` | AdminFunction | Query GSI by owner_id |
| CloudWatch Logs | `logs:*` | Both | Standard logging |

---

## Device Side (High-Level)

### Current state

- Device ID hardcoded as `"1234567890"` in `device/src/app/tasks.rs:74`
- Certs embedded at compile time from `cloud/certs/devices/test-device/` in `device/src/network/tls.rs`
- Config constant `DEVICE_ID = "supervictor"` in `device/src/config.rs:60` used only for AP portal

### Required changes

**Parameterize device ID:** Replace hardcoded `"1234567890"` with `env!("DEVICE_ID")`. Set via `.cargo/config.toml` or passed by `qs edge` before build.

**Parameterize cert paths:** Replace hardcoded `test-device` path with `env!("DEVICE_NAME")`:

```rust
// current
include_str!("../../../cloud/certs/devices/test-device/client.pem")
// target
include_str!(concat!("../../../cloud/certs/devices/", env!("DEVICE_NAME"), "/client.pem"))
```

**AP portal:** Update `build_status_json` in `device/src/network/server.rs` to use the same compile-time device ID.

### Files changed (device)

| File | Change |
|------|--------|
| `device/src/app/tasks.rs` | Replace hardcoded ID with `env!("DEVICE_ID")` |
| `device/src/network/tls.rs` | Parameterize cert paths with `env!("DEVICE_NAME")` |
| `device/src/config.rs` | Derive `DEVICE_ID` from `env!("DEVICE_ID")` |
| `device/.cargo/config.toml` | Add `DEVICE_ID` and `DEVICE_NAME` to `[env]` |

---

## CLI Integration

### Current state

`quickstart/commands/register.py` already implements `_register_device()` and `_register_owner()` but is not wired into `__main__.py`.

### Changes

**Wire register subcommand** into `quickstart/__main__.py` dispatch table:

```python
reg_p = sub.add_parser("register", help="Register devices and owners")
reg_sub = reg_p.add_subparsers(dest="target", required=True)

device_p = reg_sub.add_parser("device", help="Register a device")
device_p.add_argument("--device-id", required=True)
device_p.add_argument("--owner-id", required=True)
device_p.add_argument("--subject-dn", help="Auto-derived if omitted")
device_p.add_argument("--staging", action="store_true")
```

**Auto-derive subject_dn** when `--subject-dn` is omitted:

```python
subject_dn = args.subject_dn or f"CN={args.device_id},O=Supervictor,OU=Devices"
```

**SigV4 signing:** Admin endpoints use IAM auth. Replace bare `requests.post()` with SigV4-signed requests using `botocore` (already available via boto3, no new dependency):

```python
from botocore.auth import SigV4Auth
from botocore.awsrequest import AWSRequest
from botocore.session import Session as BotoSession
```

**IAM policy for CLI users:**

```json
{
    "Effect": "Allow",
    "Action": "execute-api:Invoke",
    "Resource": "arn:aws:execute-api:us-east-1:*:*/*/POST/devices"
}
```

---

## Testing Strategy

### Unit tests (cloud/tests/unit/)

**`test_device_validation.py`** — uplink handler validation:

| Test | Setup | Expected |
|------|-------|----------|
| validation_skipped_when_flag_false | `REQUIRE_DEVICE_REGISTRATION=false` | `(True, "")`, no DB call |
| registered_active_device_passes | Device in table, status=active | `(True, "")` |
| unregistered_device_rejected | Device not in table | `(False, "not registered")` |
| revoked_device_rejected | Device in table, status=revoked | `(False, "status is revoked")` |
| uplink_returns_403_for_unregistered | Full handler, flag=true, no device | HTTP 403 |
| uplink_returns_200_for_registered | Full handler, flag=true, device active | HTTP 200 |

**`test_admin_devices.py`** — admin handler:

| Test | Expected |
|------|----------|
| register_device_returns_201 | Valid payload creates item |
| register_device_duplicate_returns_409 | ConditionalCheckFailed → 409 |
| register_device_missing_id_returns_422 | Pydantic validation error |
| get_device_returns_200 | Known device_id returns record |
| get_device_not_found_returns_404 | Unknown device_id |
| list_devices_returns_all | All devices returned |

**Mocking:** Use `moto` to mock DynamoDB. Tests create tables, put items, invoke handlers. No external services.

```python
@pytest.fixture
def dynamodb_table():
    with moto.mock_aws():
        os.environ["DEVICES_TABLE"] = "test-devices"
        ddb = boto3.resource("dynamodb", region_name="us-east-1")
        table = ddb.create_table(
            TableName="test-devices",
            KeySchema=[{"AttributeName": "device_id", "KeyType": "HASH"}],
            AttributeDefinitions=[{"AttributeName": "device_id", "AttributeType": "S"}],
            BillingMode="PAY_PER_REQUEST",
        )
        yield table
```

### Feature flag test matrix

| REQUIRE_DEVICE_REGISTRATION | Device Registered | Expected |
|-----------------------------|-------------------|----------|
| false | yes | 200 (no DB lookup) |
| false | no | 200 (no DB lookup) |
| true | yes, active | 200 |
| true | yes, revoked | 403 |
| true | no | 403 |

### Integration tests

DynamoDB Local via Docker for register-then-uplink roundtrip:

```yaml
# cloud/docker-compose.test.yml
services:
  dynamodb-local:
    image: amazon/dynamodb-local:latest
    ports:
      - "8000:8000"
    command: ["-jar", "DynamoDBLocal.jar", "-sharedDb", "-inMemory"]
```

| Test | Flow |
|------|------|
| register_then_uplink | POST /devices → POST / → 200 |
| uplink_unregistered_rejected | POST / without registration → 403 |
| register_device_roundtrip | POST /devices → GET /devices/{id} → same data |

---

## Security

### Certificate-to-device binding

**MVP:** `subject_dn` is stored at registration and returned in responses but not enforced on uplink. mTLS already guarantees the client holds a valid CA-signed cert.

**Phase 2 enhancement:** Cross-check `client_subject` from API Gateway against the `subject_dn` in the device record to prevent a device from spoofing another device's `id` field:

```python
if client_subject and device_record.subject_dn:
    if client_subject != device_record.subject_dn:
        return 403, "Certificate does not match registered device"
```

### Device revocation

1. Update device status to `revoked` via admin API
2. Next uplink from that device gets 403
3. Optionally regenerate CA and reissue all other certs (nuclear option)

Soft revocation via status field is the practical approach.

### Secrets management

No new secrets introduced. Existing pattern (SSM for production, env vars for dev) unchanged.

---

## Implementation Sequence

### Phase A: Infrastructure (cloud/template.yaml)

1. Add `RequireDeviceRegistration` parameter
2. Add `DevicesTable` DynamoDB resource
3. Add `AdminFunction` Lambda resource with IAM-auth events
4. Add DynamoDB GetItem policy to `HelloWorldFunction`
5. Add `DEVICES_TABLE` and `REQUIRE_DEVICE_REGISTRATION` env vars to `HelloWorldFunction`
6. Add outputs for new resources
7. Validate: `sam validate --lint`

### Phase B: Admin Handler (cloud/admin/)

1. Write `cloud/tests/unit/test_admin_devices.py` (RED)
2. Create `cloud/admin/__init__.py`, `cloud/admin/app.py`, `cloud/admin/pyproject.toml`
3. Implement `RegisterDeviceRequest` model and handler routes (GREEN)
4. Generate `cloud/admin/requirements.txt` via `uv export`

### Phase C: Uplink Validation (cloud/uplink/)

1. Write `cloud/tests/unit/test_device_validation.py` (RED)
2. Add `_validate_device()` to `cloud/uplink/app.py`
3. Modify `_handle_post()` to call validation (GREEN)
4. Verify existing tests still pass with `REQUIRE_DEVICE_REGISTRATION=false`

### Phase D: CLI Wiring (quickstart/)

1. Wire `register` subcommand into `quickstart/__main__.py`
2. Add SigV4 signing to `quickstart/commands/register.py`
3. Add `--subject-dn` auto-derivation
4. Write `quickstart/tests/test_register.py`

### Phase E: Device Changes (device/)

1. Add `DEVICE_ID` and `DEVICE_NAME` env vars to `.cargo/config.toml`
2. Update `device/src/app/tasks.rs`, `tls.rs`, `config.rs`
3. Update `qs edge` to set env vars before build

### Phase F: Integration Testing

1. Create `cloud/docker-compose.test.yml` for DynamoDB Local
2. Write integration tests for register-then-uplink flow
3. Update `qs dev` to spin up DynamoDB Local alongside SAM local

---

## Files

### New

| File | Purpose |
|------|---------|
| `cloud/admin/__init__.py` | Package init |
| `cloud/admin/app.py` | Admin Lambda handler (POST/GET /devices) |
| `cloud/admin/pyproject.toml` | Dependencies (pydantic) |
| `cloud/admin/requirements.txt` | Locked deps for Lambda packaging |
| `cloud/tests/unit/test_admin_devices.py` | Unit tests for admin handler |
| `cloud/tests/unit/test_device_validation.py` | Unit tests for uplink validation |
| `cloud/docker-compose.test.yml` | DynamoDB Local for integration tests |
| `quickstart/tests/test_register.py` | Tests for register CLI command |

### Modified (minimal, additive)

| File | Change |
|------|--------|
| `cloud/template.yaml` | Add parameter, DynamoDB table, AdminFunction, policies |
| `cloud/uplink/app.py` | Add `_validate_device()`, modify `_handle_post()` |
| `cloud/samconfig.toml` | Add `RequireDeviceRegistration` to parameter overrides |
| `quickstart/__main__.py` | Wire register subcommand |
| `quickstart/commands/register.py` | Add SigV4 signing, subject_dn derivation |
| `device/src/app/tasks.rs` | Replace hardcoded device ID |
| `device/src/network/tls.rs` | Parameterize cert paths |
| `device/src/config.rs` | Derive DEVICE_ID from env |
