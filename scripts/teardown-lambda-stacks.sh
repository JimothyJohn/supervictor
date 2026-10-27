#!/usr/bin/env bash
# teardown-lambda-stacks.sh — retire the serverless path (decided 2026-10-27:
# "why are we still using Lambda when we can run this on our own naked infra").
#
# Exports each Lambda stack's two DynamoDB tables to JSON under scratch/, then
# deletes the stacks supervictor-dev and supervictor-prod: API Gateway, the
# Lambda, the DynamoDB tables, the custom domains dev.supervictor.advin.io and
# supervictor.advin.io with their ACM certificates and Route53 records.
# Idempotent: an absent stack is skipped; re-run after a failure. Deletes are
# denied to the session, so this is yours: `./scripts/teardown-lambda-stacks.sh`
# in a native terminal (or `! ./scratch/01-...` here if the prompt allows it).
#
# Left alone on purpose: s3://supervictor (truststore*.pem can go afterwards:
# `aws s3 rm s3://supervictor/truststore.pem` / truststore-dev.pem), the SAM
# managed bucket/ECR repo (aws-sam-cli-managed-default stack), the staging box.
set -euo pipefail
cd "$(dirname "$0")/.."
export AWS_DEFAULT_REGION="${AWS_REGION:-us-east-1}"
OUT="scratch/lambda-export-$(date -u +%Y%m%dT%H%M%SZ)"   # scratch/ is gitignored
mkdir -p "$OUT"

for stack in supervictor-dev supervictor-prod; do
    if ! aws cloudformation describe-stacks --stack-name "$stack" >/dev/null 2>&1; then
        echo "[$stack] absent, skipping"
        continue
    fi
    for logical in DevicesTable UplinkTable; do
        table="$(aws cloudformation describe-stack-resource --stack-name "$stack" \
            --logical-resource-id "$logical" --query StackResourceDetail.PhysicalResourceId --output text 2>/dev/null || true)"
        [ -n "$table" ] && [ "$table" != None ] || continue
        echo "[$stack] exporting $table -> $OUT/$stack-$logical.json"
        aws dynamodb scan --table-name "$table" --output json > "$OUT/$stack-$logical.json"
        python3 -I -c 'import json,sys; print("   ", json.load(open(sys.argv[1]))["Count"], "items")' "$OUT/$stack-$logical.json"
    done
    echo "[$stack] delete-stack"
    aws cloudformation delete-stack --stack-name "$stack"
done

for stack in supervictor-dev supervictor-prod; do
    aws cloudformation wait stack-delete-complete --stack-name "$stack" 2>/dev/null || true
    status="$(aws cloudformation describe-stacks --stack-name "$stack" --query 'Stacks[0].StackStatus' --output text 2>/dev/null || echo DELETED)"
    echo "[$stack] $status"
    if [ "$status" = DELETE_FAILED ]; then
        echo "  resources that refused:"
        aws cloudformation describe-stack-events --stack-name "$stack" \
            --query "StackEvents[?ResourceStatus=='DELETE_FAILED'].[LogicalResourceId,ResourceStatusReason]" --output text | sed 's/^/    /'
        echo "  fix the reason, then re-run this script (it skips what is already gone)"
    fi
done
echo "exports kept in $OUT (gitignored)."
