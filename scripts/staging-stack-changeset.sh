#!/usr/bin/env bash
# staging-stack-changeset.sh — land the rewritten template-staging.yaml on
# the live stack: the bootstrap now installs a GitHub release (no Docker, no
# Caddy, no on-box build), the IAM policy drops the bundle object, and the
# ReleaseTag / Repo / ExistingAllocationId parameters appear.
#
# Review-then-execute through scripts/staging-stack-update.sh (pins the live
# AMI; refuses a change set that would replace the instance). UserData changes
# on a running instance are "some interruptions" at most (stop/start), never a
# rebuild — but READ the change set it prints before answering y.
#
#   ./scripts/staging-stack-changeset.sh            # review, then prompt
#   YES=1 ./scripts/staging-stack-changeset.sh      # execute without prompt
#   FOLD_EIP=1 ...                                      # also hand the stack the
#       out-of-band Elastic IP (100.56.171.238, tag Name=supervictor-staging).
#       The stack's own ElasticIp resource was released by hand long ago, so
#       CloudFormation may fail to "delete" it and roll the update back; if it
#       does, run once without FOLD_EIP and leave the drift for a rebuild day.
set -euo pipefail
cd "$(dirname "$0")/.."
export AWS_DEFAULT_REGION="${AWS_REGION:-us-east-1}"
PARAMS=("ReleaseTag=${TAG:-v0.1.0}" "Repo=JimothyJohn/supervictor")
if [ "${FOLD_EIP:-0}" = 1 ]; then
    ALLOC="$(aws ec2 describe-addresses --filters Name=tag:Name,Values=supervictor-staging --query 'Addresses[0].AllocationId' --output text)"
    IP="$(aws ec2 describe-addresses --filters Name=tag:Name,Values=supervictor-staging --query 'Addresses[0].PublicIp' --output text)"
    INSTANCE="$(aws cloudformation describe-stack-resource --stack-name supervictor-endpoint-staging --logical-resource-id Instance --query StackResourceDetail.PhysicalResourceId --output text)"
    ASSOC="$(aws ec2 describe-addresses --allocation-ids "$ALLOC" --query 'Addresses[0].InstanceId' --output text)"
    [ "$ASSOC" = "$INSTANCE" ] || { echo "EIP $IP ($ALLOC) is associated with '$ASSOC', not the stack instance $INSTANCE — not folding"; exit 1; }
    PARAMS+=("ExistingAllocationId=$ALLOC" "ExistingPublicIp=$IP")
    echo "folding Elastic IP $IP ($ALLOC) into the stack"
fi
./scripts/staging-stack-update.sh "${PARAMS[@]}"
echo "verify: curl --cacert certs/ca/staging-bundle.pem --cert certs/admins/$USER/admin.pem --key certs/admins/$USER/admin.key https://staging.supervictor.advin.io/health"
