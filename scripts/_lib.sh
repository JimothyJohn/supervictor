#!/usr/bin/env bash
# _lib.sh — shared helpers for the operator scripts in this directory.
#
#   . "$(dirname "$0")/_lib.sh"        # first line after the header comment
#
# Sets REPO_ROOT, REGION (exported as AWS_DEFAULT_REGION), ADMIN with its
# ADMIN_PEM / ADMIN_KEY, and the staging stack/host names. Defines log, fail,
# need, need_admin, stack_resource, stack_output, staging_instance,
# ssm_run and admin_curl. Turns on `set -euo pipefail`.
#
#   ADMIN          admin certificate name under certs/admins/ (default: $USER)
#   AWS_REGION     region for every aws call (default: us-east-1)
#   STAGING_STACK  CloudFormation stack of the staging box (default: supervictor-endpoint-staging)
#   STAGING_HOST   its hostname (default: staging.supervictor.advin.io)
#   PROD_HOST      the production box's hostname (default: supervictor.advin.io;
#                  no box answers there yet — the Lambda stage was retired 2026-10-27)
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(git -C "$(dirname "${BASH_SOURCE[0]}")/.." rev-parse --show-toplevel)}"
REGION="${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}}"
export AWS_DEFAULT_REGION="$REGION"
ADMIN="${ADMIN:-${QS_ADMIN:-${USER:-admin}}}"
ADMIN_PEM="$REPO_ROOT/certs/admins/$ADMIN/admin.pem"
ADMIN_KEY="$REPO_ROOT/certs/admins/$ADMIN/admin.key"
STAGING_STACK="${STAGING_STACK:-supervictor-endpoint-staging}"
STAGING_HOST="${STAGING_HOST:-staging.supervictor.advin.io}"
PROD_HOST="${PROD_HOST:-supervictor.advin.io}"
# Fleet CA + ISRG Root X1: a client that trusts this bundle survives the box
# switching between a fleet-signed and a Let's Encrypt server certificate.
STAGING_BUNDLE="$REPO_ROOT/certs/ca/staging-bundle.pem"
[ -f "$STAGING_BUNDLE" ] || STAGING_BUNDLE="$REPO_ROOT/certs/ca/ca.pem"

log()  { printf '[%s] %s\n' "${LOG_TAG:-$(basename "$0" .sh)}" "$*"; }
fail() { log "ERROR: $*" >&2; exit 1; }
need() { local t; for t in "$@"; do command -v "$t" >/dev/null 2>&1 || fail "missing tool: $t"; done; }
need_admin() {
    [ -f "$ADMIN_PEM" ] && [ -f "$ADMIN_KEY" ] \
        || fail "admin cert missing at $ADMIN_PEM — ADMIN=<name>, or issue one: ./Quickstart certs admin $ADMIN"
}

# stack_resource <stack> <logical-id> -> physical id
stack_resource() {
    aws cloudformation describe-stack-resource --stack-name "$1" --logical-resource-id "$2" \
        --query StackResourceDetail.PhysicalResourceId --output text
}
# stack_output <stack> <output-key> -> value (trailing slashes stripped)
stack_output() {
    aws cloudformation describe-stacks --stack-name "$1" \
        --query "Stacks[0].Outputs[?OutputKey=='$2'].OutputValue" --output text | sed 's:/*$::'
}
staging_instance() { stack_resource "$STAGING_STACK" Instance; }
# target_host <staging|prod> -> hostname of that endpoint
target_host() {
    case "$1" in
    staging) echo "$STAGING_HOST" ;;
    prod) echo "$PROD_HOST" ;;
    *) fail "target must be staging or prod" ;;
    esac
}

# ssm_run <instance-id> <comment> <timeout-seconds> < script
# Runs the script on stdin as root on the instance (AWS-RunShellScript), waits
# for it, prints its stdout and stderr, and fails unless the remote status is
# Success. The remote side is killed at <timeout-seconds>.
ssm_run() {
    local instance="$1" comment="$2" timeout="$3" params cmd status deadline
    params="$(python3 -I -c 'import json,sys; print(json.dumps({"commands": [sys.stdin.read()], "executionTimeout": [sys.argv[1]]}))' "$timeout")"
    cmd="$(aws ssm send-command --instance-ids "$instance" --document-name AWS-RunShellScript \
        --comment "$comment" --parameters "$params" --query 'Command.CommandId' --output text)"
    log "ssm command $cmd on $instance — waiting (up to ${timeout}s)"
    deadline=$((SECONDS + timeout + 60))
    while :; do
        status="$(aws ssm get-command-invocation --command-id "$cmd" --instance-id "$instance" \
            --query Status --output text 2>/dev/null || echo Pending)"
        case "$status" in Pending | InProgress | Delayed) ;; *) break ;; esac
        [ "$SECONDS" -lt "$deadline" ] || { log "gave up waiting on $cmd (last status $status)"; return 1; }
        sleep 5
    done
    aws ssm get-command-invocation --command-id "$cmd" --instance-id "$instance" \
        --query '[StandardOutputContent,StandardErrorContent]' --output text
    [ "$status" = Success ] || { log "remote status: $status"; return 1; }
}

# admin_curl <curl args> — curl with the admin identity (add --cacert yourself).
admin_curl() { curl -sS --cert "$ADMIN_PEM" --key "$ADMIN_KEY" "$@"; }
