# Orchestration Pitch

How supervictor could grow beyond serverless — and when it should.

## Where We Are Now

The current architecture is clean and appropriate for its scale:

```
ESP32-C3 ──mTLS──► API Gateway ──► Lambda (single handler)
```

- One Lambda function, one endpoint, stateless request/response
- SAM/CloudFormation manages infrastructure as code
- `qs` CLI orchestrates the dev/staging/prod pipeline locally
- Zero long-running services, zero ops burden

**This works.** Don't change it until something forces you to.

## Where Orchestration Starts to Matter

The enterprise roadmap (`todo/ENTERPRISE.md`) adds:

- 3 DynamoDB tables (devices, messages, owners)
- 3+ Lambda functions (ingest, management, billing-sync)
- Scheduled jobs (billing-sync via CloudWatch Events)
- Stripe API integration with secret management
- Device provisioning workflows (register → issue cert → activate)
- Cert rotation lifecycle

Once you have multiple functions coordinating across tables, schedules, and external APIs, you're running a distributed system. SAM deploys it, but doesn't orchestrate it — you need something that understands workflows, dependencies, and failure recovery.

## Three Layers of Orchestration

"Orchestration" isn't one thing. There are three distinct layers, each with different tools and triggers.

### Layer 1: Workflow Orchestration

**Problem**: Multi-step operations that span services and need retry/rollback logic.

**Examples**:
- Device provisioning: validate owner → generate cert → write to devices table → upload truststore → return cert bundle
- Billing sync: query devices per owner → count messages → report to Stripe → update last-sync timestamp
- Cert rotation: generate new cert → deploy to device OTA → revoke old cert → update truststore

**Tool**: AWS Step Functions (or Temporal for vendor-neutral)

**Why**: These are stateful, multi-step workflows where any step can fail. Step Functions gives you visual debugging, automatic retries, parallel branches, and wait states. Writing this as Lambda-calling-Lambda with try/except is a recipe for silent data corruption.

**What changes**:
- `template.yaml`: Add `AWS::Serverless::StateMachine` resources alongside the Lambdas
- Each Lambda becomes a single-purpose step (validate, write, notify) instead of a monolith
- `qs staging` gains a Step Functions integration test
- Error handling moves from application code to the state machine definition

### Layer 2: Service Orchestration

**Problem**: Some workloads don't fit the Lambda model (15-min timeout, cold starts, no persistent connections).

**Examples**:
- MQTT broker for real-time device communication (persistent TCP connections)
- WebSocket server for live dashboard updates
- Device state machine tracking online/offline/error states
- Stream processor consuming from DynamoDB Streams or Kinesis

**Tool**: ECS Fargate first, EKS (Kubernetes) when you outgrow it.

| Criteria | ECS Fargate | EKS (Kubernetes) |
|----------|-------------|-------------------|
| Ops overhead | Low — AWS manages the cluster | High — you manage the control plane |
| Learning curve | Small if you know Docker | Steep — YAML, Helm, operators, RBAC |
| Cost at low scale | Pay per task, scales to zero | ~$75/mo minimum for control plane |
| Multi-cloud | No | Yes |
| Namespace isolation | Task definitions | Native namespaces |
| Custom operators | No | Yes — CRDs for device lifecycle |
| GitOps | Basic (task def versioning) | Full (ArgoCD/Flux) |

**When to move**:
- Fargate: When you need a single long-running service (MQTT broker, WebSocket server)
- EKS: When you need 3+ long-running services, multi-tenant isolation, or custom operators

**What changes**:
- New `services/` directory alongside `cloud/` for containerized services
- Each service gets a Dockerfile (multi-stage, distroless base — per project standards)
- `qs` gains `qs services` commands to deploy/manage containers
- mTLS cert distribution shifts from compile-time embedding to runtime secret injection (AWS Secrets Manager → sidecar)

### Layer 3: Edge Orchestration

**Problem**: ESP32 devices are constrained — no local storage, no aggregation, no offline buffering. A gateway device between ESP32s and the cloud solves this.

**Example architecture**:
```
ESP32-C3 ──BLE/WiFi──► Gateway (RPi) ──mTLS──► Cloud API
                           │
                           ├── Local buffer (SQLite)
                           ├── Aggregation / filtering
                           ├── OTA update server
                           └── Local dashboard
```

**Tool**: K3s (lightweight Kubernetes) or Balena (purpose-built for IoT fleets)

| Criteria | K3s | Balena |
|----------|-----|--------|
| Target | General edge computing | IoT device fleets specifically |
| Runtime | Full K8s API, ~50MB binary | Docker + fleet management |
| OTA updates | Helm charts / GitOps | Built-in, atomic |
| Fleet management | Custom (or Rancher) | Built-in dashboard |
| Offline support | Yes | Yes |
| Learning curve | K8s knowledge required | Docker-only, simpler |

**When to move**: When you have 10+ devices in a single location and need local aggregation, offline buffering, or sub-second response times that cloud round-trips can't deliver.

**What changes**:
- New `gateway/` directory for gateway services
- Gateway runs containerized services: BLE scanner, message buffer, cloud uplink
- `qs` gains `qs gateway` commands for fleet deployment
- Cert management adds gateway certificates to the trust hierarchy

## Why Not Jump Straight to K8s

Kubernetes solves real problems — at the cost of real complexity:

- **Cluster management**: Even EKS requires node group sizing, networking (VPC CNI), and RBAC policies
- **YAML sprawl**: Deployments, Services, Ingresses, ConfigMaps, Secrets, HPA, PDB — each service needs 5+ manifests
- **Debugging**: `kubectl logs` + `kubectl describe` + `kubectl exec` replaces CloudWatch's single pane
- **Cost floor**: EKS control plane is ~$75/month before any workload runs
- **Team expertise**: One engineer managing K8s is a full-time job

The current stack has zero ops burden. Lambda scales to zero, SAM deploys in one command, `qs` handles the pipeline. Adding K8s before you need it trades this simplicity for infrastructure toil.

## Recommended Progression

### Phase 0: Now (1-10 devices)
**Keep serverless.** Add Step Functions when the enterprise roadmap lands.

```
ESP32 ──mTLS──► API GW ──► Lambda
                              ├──► DynamoDB
                              └──► Step Functions (provisioning, billing)
```

- Trigger: Enterprise roadmap implementation
- Add to `template.yaml`: `AWS::Serverless::StateMachine` definitions
- `qs` unchanged — SAM handles Step Functions deployment

### Phase 1: Real-Time (10-100 devices)
**Add ECS Fargate** for the first long-running service (MQTT or WebSocket broker).

```
ESP32 ──mTLS──► API GW ──► Lambda (ingest, mgmt)
ESP32 ──MQTT──► Fargate (broker) ──► DynamoDB Streams
Dashboard  ◄──WS──► Fargate (dashboard-api)
```

- Trigger: Need for persistent connections or real-time data
- New: `services/mqtt-broker/` with Dockerfile
- `template.yaml` adds `AWS::ECS::Service` or separate CDK stack
- `qs` gains `qs services deploy`

### Phase 2: Multi-Tenant (100+ devices)
**Evaluate EKS** when you need tenant isolation and custom automation.

```
Tenant A ──► K8s namespace A ──► shared data plane
Tenant B ──► K8s namespace B ──► shared data plane
                                    ├──► DynamoDB
                                    ├──► Stripe
                                    └──► S3
```

- Trigger: Multiple customers with data isolation requirements
- K8s namespaces provide network policies, resource quotas, RBAC per tenant
- Custom CRDs for device lifecycle: `kubectl apply -f device.yaml` creates cert + DB record + Stripe subscription
- GitOps via ArgoCD: push to `deploy/` triggers rollout

### Phase 3: Field Gateways (site installations)
**Add K3s** on gateway hardware for local aggregation.

- Trigger: Latency-sensitive use cases or unreliable internet at device sites
- Gateway runs BLE scanner + local buffer + cloud uplink as containers
- Fleet management via Rancher or Balena
- `qs gateway flash <site>` provisions a new gateway

## The Portable Path

The project already values containerization. To keep future options open:

1. **Structure all new services as containers from day one** — even if they deploy as Lambda. Use the Lambda Web Adapter pattern: a standard HTTP server that runs in Lambda via a shim, or in Fargate/K8s as-is.

2. **Keep business logic framework-agnostic** — handler functions take a request, return a response. The Lambda/Fargate/K8s wrapper is a thin adapter.

3. **Externalize configuration** — environment variables, not hardcoded AWS SDK calls. The same container runs locally, in Fargate, or in K8s with different env vars.

4. **Container-first CI** — test against the container image, not the Lambda deployment package. `qs dev` already uses SAM's container-based local testing; extend this pattern.

This way, the Lambda → Fargate migration is a deployment config change (task definition instead of SAM function), and Fargate → K8s is another config change (Kubernetes manifests instead of task definitions). No application rewrites at any stage.

## Summary

| Stage | Trigger | Tool | Ops Cost |
|-------|---------|------|----------|
| Now | Enterprise features land | Step Functions | Near zero |
| 10-100 devices | Real-time requirement | ECS Fargate | Low |
| 100+ devices | Multi-tenant isolation | EKS | Medium |
| Site installations | Local aggregation needed | K3s / Balena | Per-site |

Don't orchestrate what doesn't need orchestrating. Add each layer when the pain of not having it exceeds the cost of running it.
