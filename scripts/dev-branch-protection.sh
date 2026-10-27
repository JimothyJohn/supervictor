#!/usr/bin/env bash
# dev-branch-protection.sh — make the DynamoDB contract job a required
# check on `dev`, so a dynamo-only failure blocks auto-merge (P1 card). The
# agent session's attempt was denied by the permission classifier (CI bypass
# category), so Nick runs it (TODO.md).
# Idempotent: PATCHes the required-status-checks list to exactly these three.
set -euo pipefail
REPO=JimothyJohn/supervictor
echo "before:"
gh api "repos/$REPO/branches/dev/protection/required_status_checks" --jq '.contexts'
gh api -X PATCH "repos/$REPO/branches/dev/protection/required_status_checks" --input - <<'EOF' >/dev/null
{"strict": false, "contexts": ["Lint + test (host)", "Embedded build (riscv32, ESP32-C3)", "DynamoDB store (DynamoDB Local)"]}
EOF
echo "after:"
gh api "repos/$REPO/branches/dev/protection/required_status_checks" --jq '.contexts'
