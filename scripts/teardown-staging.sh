#!/usr/bin/env bash
# teardown-staging.sh — delete the endpoint box stack of a stage.
#
#   ./scripts/teardown-staging.sh                                   # staging
#   STAGE=prod CONFIRM=supervictor-endpoint-prod ./scripts/teardown-staging.sh
#
# Deletes the CloudFormation stack and waits. The instance goes with it, and
# so does its SQLite file, Let's Encrypt cert and installed binary — there is
# no backup step here. For prod the stack name must be repeated in CONFIRM.
# The AMI pin at /supervictor/<stage>/ami-id is left in place (harmless, and
# a rebuild wants the same AMI).
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=teardown
STACK_NAME="${STACK_NAME:-$STAGING_STACK}"
need aws

if [ "$STAGE" = prod ] && [ "${CONFIRM:-}" != "$STACK_NAME" ]; then
    fail "refusing to delete the PROD box: set CONFIRM=$STACK_NAME to proceed"
fi
aws cloudformation describe-stacks --stack-name "$STACK_NAME" >/dev/null 2>&1 || fail "no stack named $STACK_NAME in $REGION"

log "deleting stack $STACK_NAME in $REGION"
aws cloudformation delete-stack --stack-name "$STACK_NAME"
log "waiting for the delete to complete"
aws cloudformation wait stack-delete-complete --stack-name "$STACK_NAME"
log "done"
