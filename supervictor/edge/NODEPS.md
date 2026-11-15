# Dependencies that could go

Whole-workspace review, 2026-10-28, after the Lambda retirement and the prod
cutover. The bias is to keep a dep: removal has to earn it, and no swap lands
without a test that pins the replaced behavior. Each item is independent, and
none is done yet. They are proposals, smallest first. This replaces the earlier
edge-only plan (five "levels"). Its outcomes are recorded under **Kept** below.

Crate counts are `cargo tree --workspace --target all -e normal,dev,build`
unique packages: 514 with every feature on, 445 without `dynamo`.

## Unused: delete

1. **`supervictor-mcp`: `serde`.** Nothing in `mcp/src` imports it or
   derives with it. Every use is `serde_json::{json, Value, Map}`.
   Remove the line; serde stays in the tree through serde_json.
2. **`supervictor-mcp`: `rustls-pki-types`.** Zero uses. `client.rs` reaches
   `PemObject`, `CertificateDer`, `PrivateKeyDer`, `ServerName` through
   `rustls::pki_types`. Remove the line; rustls's `std` feature already turns
   on pki-types' `std`, which `PemObject::from_pem_file` needs.
   Verify both with `cargo clippy -p supervictor-mcp --all-targets -- -D warnings`
   and the mcp end-to-end test.

## Replace

3. **`supervictor-edge`: `rustls-pemfile`** (`src/bin/desktop_main.rs:43`,
   `certs()` + `private_key()` behind two `BufReader`s). `cargo audit` flags it
   **unmaintained**. Its parsing now lives in `rustls-pki-types`, which the
   endpoint (`src/tls.rs`) and the mcp (`src/client.rs`) already use:
   `CertificateDer::pem_slice_iter` and `PrivateKeyDer::from_pem_slice`
   (the `_slice` twins of the `_file` calls those two make).
   No hand-written PEM/base64 (the old plan's 80 lines). This is not
   native code; it moves to a crate already in the tree. Test: a desktop-feature
   unit test loading a cert chain + PKCS#8 and SEC1 keys from
   `rcgen`-generated PEM, green before and after.
4. **`supervictor-edge` dev-deps: `rustls` / `tokio-rustls` with default
   features.** The normal deps carefully pick `ring`, but the dev-dep
   lines take defaults, so every host `cargo test --workspace` also compiles
   `aws-lc-rs` + `aws-lc-sys` (a C/cmake build). Workspace feature
   unification puts it under the endpoint and mcp tests too. `rcgen` is on
   `ring` already. Fix: the dev-dep lines mirror the normal ones
   (`default-features = false, features = ["ring", "logging", "std", "tls12"]`).
   Test: the existing `tests/tls_failure_handling.rs` and the endpoint/mcp
   TLS suites; `cargo tree -i aws-lc-rs` must come back empty.

## Services and features

5. **Endpoint `dynamo` feature** (`aws-sdk-dynamodb`, `aws-config`).
   It was the Lambda's backend. No deployment uses it now: both boxes run
   `STORE_BACKEND=sqlite` (`scripts/install-endpoint.sh`), and the release
   artifact is built without it (`release.yml`). It costs about 70 crates (514 → 445),
   including the `h2 0.3` and `ctutils` paths in the red `cargo audit`. It also
   costs a CI job against DynamoDB Local and the open "make the DynamoDB job a
   required check" card. Removing it means: `src/store/dynamo.rs`,
   `tests/test_store_dynamo.rs`, the factory/config/error arms,
   `UPLINK_TTL_DAYS`, the `dynamo` CI job, `scripts/dev-branch-protection.sh`
   and its card, plus doc lines (README, TELEMETRY.md, endpoint CLAUDE.md,
   bring-your-own server.md). Keep it instead if a multi-box or managed-DB
   future (`todo/ENTERPRISE.md`) is meant to start from it. **Nick's call.**
6. **AWS resources the Lambda teardown left.** The SAM ECR companion stacks,
   the API Gateway truststores in S3, a CloudFront distribution still aliased
   to supervictor.advin.io with the deleted execute-api as its origin, its ACM
   cert, and two ACM validation CNAMEs. Run `scripts/teardown-lambda-leftovers.sh`
   (TODO.md card `teardown-lambda-leftovers`).

## Lambda-era code, not deps

- `LambdaResponse` in `src/models/uplink.rs` still has three fields that hold
  API Gateway headers (`x-amzn-requestid`, `x-amz-apigw-id`,
  `x-amzn-trace-id`); `parse_response` fills them and the box never sends
  them. Renaming the type and dropping the fields is a firmware change with
  a test sweep (`uplink_tests.rs`, `http_gap_tests.rs`, `http_edge_tests.rs`).
  Ship it with the next OTA, not alone.
- `mcp/src/config.rs` and the OTA `manifest_request` (pinned by
  `src/ota/http_tests.rs`) fold a raw execute-api stage prefix into paths. The behavior still works behind any path-prefixed
  proxy, so it stays. Only the comments name API Gateway.

## Kept, with the reason

- **`embedded-io-async`.** The old plan's "never imported" no longer holds:
  `app/tasks.rs` and `ota/device.rs` are generic over its `Read + Write`.
- **`static_cell`.** The old plan's `static mut` + `MaybeUninit` replacement
  drops the double-init guard, and `static mut` references are denied in
  edition 2024. Kept.
- **`tokio-rustls`.** The endpoint and the mcp need it regardless, so
  dropping it from edge removes no crate.
- **`serde` + `serde-json-core`.** The device now parses `UplinkAck` from the
  server (`#[serde(default)]`, optional fields). That is JSON from the network,
  which is load-bearing. The old plan's hand-written extractor predates it.
- **`heapless`, `sha2`, `esp32`, `embassy-net-driver`, `embedded-storage`.**
  Each is load-bearing and has its reason in `Cargo.toml`.
- **The Docker + Caddy path** (`endpoint/Dockerfile`, `docker-compose.yml`,
  `Caddyfile`, and the standalone `endpoint/Cargo.lock` the Dockerfile
  builds from). It is path C in the bring-your-own skill. CI never builds it, so it
  can rot, but that's a test gap, not dead weight.
