# Supervictor Enterprise Upgrade

Adds device registry, data persistence, usage-based billing via Stripe.

## Architecture

```
ESP32-C3 ──mTLS──► API Gateway ──► Lambda (ingest)
                                      ├──► DynamoDB (messages)
                                      └──► DynamoDB (devices) [validate]

CloudWatch Schedule ──► Lambda (billing-sync)
                            ├──► DynamoDB (read counts)
                            └──► Stripe API (report usage)

Admin/Owner ──► API Gateway ──► Lambda (management)
                                  ├──► DynamoDB (owners, devices)
                                  └──► Stripe (customers, portal)
```

## Data Model

### Devices Table
| Field      | Type   | Notes                        |
|------------|--------|------------------------------|
| device_id  | PK     | Unique device identifier     |
| owner_id   | String | FK to owners table, GSI      |
| subject_dn | String | mTLS certificate subject DN  |
| status     | String | active / revoked             |
| created_at | String | ISO 8601                     |

### Messages Table
| Field     | Type   | Notes                        |
|-----------|--------|------------------------------|
| device_id | PK     | FK to devices table          |
| timestamp | SK     | ISO 8601, sort key           |
| current   | Number | Sensor reading               |

### Owners Table
| Field                  | Type   | Notes                      |
|------------------------|--------|----------------------------|
| owner_id               | PK     | UUID                       |
| email                  | String | Contact / Stripe email     |
| stripe_customer_id     | String | Stripe customer object ID  |
| stripe_subscription_id | String | Metered subscription ID    |
| created_at             | String | ISO 8601                   |

## API Surface

| Method | Path                    | Auth    | Purpose                          |
|--------|-------------------------|---------|----------------------------------|
| POST   | /hello                  | mTLS    | Device uplink (existing, add DB write) |
| GET    | /hello                  | none    | Health check (existing)          |
| POST   | /devices                | IAM     | Register a device                |
| GET    | /devices                | IAM     | List all devices                 |
| GET    | /devices/{id}           | IAM     | Device status + message count    |
| POST   | /owners                 | IAM     | Create owner + Stripe customer   |
| GET    | /owners/{id}            | IAM     | Owner info + device list         |
| POST   | /owners/{id}/portal     | IAM     | Generate Stripe billing portal URL |

Scheduled (no endpoint):
- **billing-sync**: CloudWatch Events -> Lambda, reports per-owner message counts to Stripe.

## Stripe Model

- **Product:** "Supervictor Data Ingestion"
- **Price:** Metered, per unit = 1,000 messages, billed monthly
- Each owner gets a Stripe Customer and a Subscription with the metered price
- `billing-sync` Lambda reads message counts since last sync, reports usage records
- Owners access invoices/payment via Stripe billing portal (hosted by Stripe)

## Implementation Phases

### Phase 1: DynamoDB Tables
Add three tables to `cloud/template.yaml` as `AWS::DynamoDB::Table` resources.
- Devices: simple PK (`device_id`), GSI on `owner_id`
- Messages: composite key (`device_id` + `timestamp`)
- Owners: simple PK (`owner_id`)
- Use PAY_PER_REQUEST billing mode (scales to zero)

### Phase 2: Ingest Persistence
Update `POST /hello` Lambda handler:
1. Validate `device_id` exists in devices table and status is `active`
2. Write message to messages table with server-generated timestamp
3. Return response (existing shape, maybe add `stored: true`)

No Stripe dependency in this path. Keep it fast.

### Phase 3: Device Registration
New Lambda function or path for `POST /devices`:
1. Accept `{ device_id, owner_id, subject_dn }`
2. Write to devices table
3. Add `qs register-device` CLI command to quickstart

### Phase 4: Owner Management
New Lambda function or path for `POST /owners`:
1. Accept `{ owner_id, email }`
2. Create Stripe Customer via API
3. Create Stripe Subscription with metered price
4. Write to owners table with Stripe IDs
5. Add `qs register-owner` CLI command

### Phase 5: Billing Sync
Scheduled Lambda (CloudWatch Events rule, e.g. daily):
1. For each owner, query all their devices (GSI on devices table)
2. Count messages since last sync (query messages table per device)
3. Report usage to Stripe: `stripe.SubscriptionItem.create_usage_record()`
4. Store last sync timestamp (in owners table or a separate field)

### Phase 6: Billing Portal
New endpoint `POST /owners/{id}/portal`:
1. Look up owner's `stripe_customer_id`
2. Call `stripe.billing_portal.Session.create()`
3. Return portal URL

## Testing Strategy

### Unit Tests
- DynamoDB operations mocked with in-memory dict-backed store
- Stripe API mocked with request/response fixtures
- Validate all Pydantic models for new request/response shapes

### Integration Tests (qs dev)
- SAM local with DynamoDB Local container
- POST /hello writes to local DynamoDB, verify with scan
- Device registration + lookup roundtrip
- Owner creation with mocked Stripe (or Stripe test mode)

### Staging Tests (qs staging)
- Deploy to dev stack with real DynamoDB tables
- End-to-end uplink + persistence
- Stripe test mode keys for billing flow

### Production Tests (qs prod)
- mTLS device uplink + verify storage
- Stripe live mode (use low-value test transactions)

## Security

- Stripe secret key stored in AWS SSM Parameter Store (SecureString)
- Management endpoints (devices, owners) use IAM auth, not public
- Device uplink remains mTLS-only
- Stripe webhook signature verification if webhooks are added later
- Never log or store Stripe secret keys

## Resources

### AWS
- [SAM DynamoDB Table](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-resource-simpletable.html)
- [DynamoDB CloudFormation Resource](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-resource-dynamodb-table.html)
- [DynamoDB boto3 Table](https://boto3.amazonaws.com/v1/documentation/api/latest/reference/services/dynamodb/table/index.html)
- [DynamoDB GSI](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GSI.html)
- [DynamoDB Local for Testing](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/DynamoDBLocal.html)
- [CloudWatch Events Schedule](https://docs.aws.amazon.com/AmazonCloudWatch/latest/events/ScheduledEvents.html)
- [SAM Schedule Event](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/sam-property-function-schedule.html)
- [SSM Parameter Store](https://docs.aws.amazon.com/systems-manager/latest/userguide/systems-manager-parameter-store.html)
- [API Gateway IAM Auth](https://docs.aws.amazon.com/apigateway/latest/developerguide/permissions.html)

### Stripe
- [Stripe Python SDK](https://github.com/stripe/stripe-python)
- [Metered Billing / Usage Records](https://docs.stripe.com/billing/subscriptions/usage-based)
- [Create a Customer](https://docs.stripe.com/api/customers/create)
- [Create a Subscription](https://docs.stripe.com/api/subscriptions/create)
- [Report Usage Records](https://docs.stripe.com/api/usage_records/create)
- [Billing Portal](https://docs.stripe.com/billing/subscriptions/integrations/customer-portal)
- [Test Mode & Test Clocks](https://docs.stripe.com/testing)
- [Webhook Signature Verification](https://docs.stripe.com/webhooks/signatures)
- [Stripe CLI for Local Testing](https://docs.stripe.com/stripe-cli)

### Python
- [Pydantic v2 Models](https://docs.pydantic.dev/latest/)
- [boto3 DynamoDB Conditions](https://boto3.amazonaws.com/v1/documentation/api/latest/reference/customizations/dynamodb.html#ref-valid-dynamodb-conditions)
