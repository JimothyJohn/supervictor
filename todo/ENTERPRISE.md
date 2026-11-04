# Supervictor Enterprise Upgrade

Adds owners and usage-based billing via Stripe on top of the device registry
the endpoint already has. Nothing here is scheduled (`ROADMAP.md`,
"Explicitly deferred"). The infrastructure is designed on the single-binary
path (one endpoint, a SQLite file, admin mTLS) when this track opens.

## Already in the endpoint

- **Device registry and persistence.** `DeviceStore`
  (`supervictor/endpoint/src/store/`) keeps devices (`device_id`,
  `owner_id`, `subject_dn`, `status` active / revoked, `created_at`) and
  every uplink, in SQLite by default or DynamoDB behind the `dynamo`
  feature.
- **Registration and ingest validation.** `POST /devices`, `GET /devices`,
  `GET /devices/{id}` (README, "Build on it"); an uplink is bound to its
  registered certificate, and `REQUIRE_REGISTRATION=true` turns away
  unregistered, inactive or unbound devices.
- **Onboarding.** `./Quickstart onboard` and `scripts/rack-flash.sh`
  (cert → register → flash → verify).
- **Management auth.** Admin routes need a client certificate with
  `OU=admin`.

## What this track adds

### Owners

| Field                  | Type   | Notes                         |
|------------------------|--------|-------------------------------|
| owner_id               | PK     | matches `owner_id` on devices |
| email                  | String | Contact / Stripe email        |
| stripe_customer_id     | String | Stripe customer object ID     |
| stripe_subscription_id | String | Metered subscription ID       |
| created_at             | String | ISO 8601                      |

| Method | Path                | Auth  | Purpose                            |
|--------|---------------------|-------|------------------------------------|
| POST   | /owners             | admin | Create owner + Stripe customer     |
| GET    | /owners/{id}        | admin | Owner info + device list           |
| POST   | /owners/{id}/portal | admin | Generate Stripe billing portal URL |

Owner-scoped views of the dashboard are the UI half (`ROADMAP.md`,
endpoint track).

### Stripe model

- **Product:** "Supervictor Data Ingestion"
- **Price:** Metered, per unit = 1,000 messages, billed monthly
- Each owner gets a Stripe Customer and a Subscription with the metered price
- A scheduled billing sync counts each owner's uplinks since the last sync
  and reports usage records
- Owners access invoices/payment via Stripe billing portal (hosted by Stripe)

### Phases

1. **Owner management** — `POST /owners`: accept `{ owner_id, email }`,
   create the Stripe Customer and the Subscription with the metered price,
   store the owner with its Stripe IDs.
2. **Billing sync** — on a schedule (e.g. daily): for each owner, count
   uplinks across their devices since the last sync, report usage to
   Stripe, store the last sync timestamp.
3. **Billing portal** — `POST /owners/{id}/portal`: look up the owner's
   `stripe_customer_id`, create a billing portal session, return its URL.

No Stripe dependency on the uplink path. Keep it fast.

## Testing

- Store changes run the shared `DeviceStore` contract suite against every
  backend.
- Stripe in test mode, with test clocks for the monthly cycle.

## Security

- Stripe secret key stored in AWS SSM Parameter Store (SecureString),
  resolved at runtime
- Management routes stay admin mTLS; device uplink remains mTLS-only
- Stripe webhook signature verification if webhooks are added later
- Never log or store Stripe secret keys

## Resources

- [SSM Parameter Store](https://docs.aws.amazon.com/systems-manager/latest/userguide/systems-manager-parameter-store.html)
- [Metered Billing / Usage Records](https://docs.stripe.com/billing/subscriptions/usage-based)
- [Create a Customer](https://docs.stripe.com/api/customers/create)
- [Create a Subscription](https://docs.stripe.com/api/subscriptions/create)
- [Report Usage Records](https://docs.stripe.com/api/usage_records/create)
- [Billing Portal](https://docs.stripe.com/billing/subscriptions/integrations/customer-portal)
- [Test Mode & Test Clocks](https://docs.stripe.com/testing)
- [Webhook Signature Verification](https://docs.stripe.com/webhooks/signatures)
- [Stripe CLI for Local Testing](https://docs.stripe.com/stripe-cli)
