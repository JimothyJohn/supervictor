# supervictor

IoT platform: ESP32-C3 edge device with mTLS uplink to AWS Lambda.

## Monorepo Layout
- `device/` — Rust/Embassy firmware (no_std, riscv32imc + desktop dual-target)
- `cloud/` — Python/SAM API (Lambda, API Gateway, mTLS)
- `quickstart/` — Python CLI orchestrator (`python3 -m quickstart <command>`)

## Quickstart CLI
```
qs dev        # Rust tests + Python unit tests + SAM local integration tests
qs edge       # Build and flash ESP32-C3 firmware
qs staging    # Dev gate + deploy to dev stack + remote tests
qs prod       # Full pipeline + confirmation + production deploy
```

## Environment
- `.env.dev`, `.env.staging` — local env files (gitignored)
- Certs generated via `cloud/scripts/gen_certs.sh` (gitignored)
- CI: `.github/workflows/python_ci.yml`, `.github/workflows/rust_ci.yml`

## Safety
- Never commit `.env*`, `*.pem`, `*.key`, `*.crt`, or anything in `certs/`
- Production deploy requires explicit `qs prod` confirmation
- mTLS truststore lives at `s3://supervictor/truststore.pem`
