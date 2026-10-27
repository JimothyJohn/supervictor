#!/usr/bin/env bash
# staging-stack-update.sh — update a box's CloudFormation stack WITHOUT rebuilding the box.
#
#   ./scripts/staging-stack-update.sh [Key=Value ...]            # review the change set
#   YES=1 ./scripts/staging-stack-update.sh [Key=Value ...]      # ...and execute it
#   STAGE=prod ./scripts/staging-stack-update.sh InstanceType=t4g.small   # the prod box
#
# Why not plain `aws cloudformation deploy`: the template's AmiId default is
# the "latest AL2023" SSM alias, which moves, and an update that re-resolves
# it REPLACES the instance — losing the SQLite file, the Let's Encrypt cert
# and any hand-installed build. The live AMI is pinned in SSM at
# /supervictor/<stage>/ami-id (AMI_PARAM from _lib.sh; deploy-staging.sh pins
# it at creation) and this script always passes that;
# every other parameter keeps its current stack value unless given on the
# command line (deploy's UsePreviousValue semantics).
#
# It creates the change set without executing it, prints every resource it
# touches, refuses if the Instance would be replaced or removed
# (ALLOW_REPLACE=1 overrides), and executes only with YES=1 or a `y` at the
# prompt (a `!` run from an agent session has no stdin: use YES=1). Template
# changes land via a reviewed PR first; this is the hand-executed step.
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=stack
TEMPLATE="$REPO_ROOT/supervictor/endpoint/template-staging.yaml"
need aws
cd "$REPO_ROOT"
aws ssm get-parameter --name "$AMI_PARAM" --query Parameter.Value --output text >/dev/null \
    || fail "SSM parameter $AMI_PARAM missing — pin the live AMI there first: aws ssm put-parameter --name $AMI_PARAM --type String --value <ami-id>"

log "creating a change set for $STAGING_STACK (AmiId=$AMI_PARAM${*:+ $*})"
aws cloudformation deploy --template-file "$TEMPLATE" --stack-name "$STAGING_STACK" \
    --capabilities CAPABILITY_IAM --no-execute-changeset --no-fail-on-empty-changeset \
    --parameter-overrides "AmiId=$AMI_PARAM" "$@" >/dev/null
CS="$(aws cloudformation list-change-sets --stack-name "$STAGING_STACK" \
    --query 'sort_by(Summaries, &CreationTime)[-1].ChangeSetName' --output text)"
[ -n "$CS" ] && [ "$CS" != None ] || { log "no changes to deploy"; exit 0; }
STATUS="$(aws cloudformation describe-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS" \
    --query '[Status,StatusReason]' --output text)"
case "$STATUS" in
FAILED*) log "no changes to deploy ($STATUS)"; aws cloudformation delete-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS"; exit 0 ;;
esac

log "change set $CS touches:"
aws cloudformation describe-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS" \
    --query 'Changes[].ResourceChange.[Action,LogicalResourceId,ResourceType,Replacement]' --output text | sed 's/^/  /'
INSTANCE_HIT="$(aws cloudformation describe-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS" \
    --query "length(Changes[?ResourceChange.LogicalResourceId=='Instance' && (ResourceChange.Action=='Remove' || ResourceChange.Replacement=='True' || ResourceChange.Replacement=='Conditional')])" --output text)"
if [ "$INSTANCE_HIT" != 0 ] && [ "${ALLOW_REPLACE:-}" != 1 ]; then
    aws cloudformation delete-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS"
    fail "the change set would replace or remove the Instance; refusing (ALLOW_REPLACE=1 to override — the box's state is lost)"
fi

if [ "${YES:-}" = 1 ]; then log "YES=1 — executing"; else
    [ -t 0 ] || { log "not executing: no terminal and YES=1 not set (change set $CS left for review)"; exit 1; }
    read -r -p "execute? [y/N] " a; [ "$a" = y ] || { log "aborted (change set $CS left for review)"; exit 1; }
fi
aws cloudformation execute-change-set --stack-name "$STAGING_STACK" --change-set-name "$CS"
aws cloudformation wait stack-update-complete --stack-name "$STAGING_STACK"
log "stack: $(aws cloudformation describe-stacks --stack-name "$STAGING_STACK" --query 'Stacks[0].StackStatus' --output text)"
log "instance: $(aws ec2 describe-instances --instance-ids "$(staging_instance)" --query 'Reservations[0].Instances[0].[InstanceId,State.Name,PublicIpAddress]' --output text)"
