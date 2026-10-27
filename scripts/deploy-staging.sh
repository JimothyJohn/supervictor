#!/usr/bin/env bash
# deploy-staging.sh — stand up (or re-create) the staging box: one EC2 instance
# that installs a GitHub release of the single-binary endpoint on first boot.
#
#   TAG=v0.2.0 ./scripts/deploy-staging.sh
#
# What it does:
#   1. issues an ECDSA P-256 server certificate for the hostname from the fleet
#      CA (certs/ca/ca.key, kept on this machine only) — unless FORCE_CERT=0 and
#      one is already in S3; staging-letsencrypt.sh swaps in a public one later
#   2. uploads ca.pem / server.pem / server.key to s3://$BUCKET/staging/
#   3. deploys supervictor/endpoint/template-staging.yaml; the instance's
#      bootstrap fetches the PEMs and runs scripts/install-endpoint.sh $TAG
#
# For a stack that already exists use scripts/staging-stack-update.sh instead:
# it pins the live AMI so the instance (SQLite file, Let's Encrypt cert, any
# hand-installed build) is never replaced. This script is for a fresh stack.
#
#   TAG               release the box installs (default: the newest GitHub release)
#   STACK_NAME        default supervictor-endpoint-staging
#   HOSTNAME_FQDN     default staging.supervictor.advin.io
#   INSTANCE_TYPE     default t4g.micro
#   HOSTED_ZONE_ID    Route53 zone for the hostname
#   BUCKET            S3 bucket for the PEMs (default supervictor)
#   EIP_ALLOCATION_ID / EIP_PUBLIC_IP
#                     reuse an Elastic IP allocated outside the stack (both or neither)
#   FORCE_CERT=0      keep the server cert already in S3
#
# Needs: aws, openssl, gh (for the default TAG).
# shellcheck source=scripts/_lib.sh
. "$(dirname "$0")/_lib.sh"
LOG_TAG=deploy
STACK_NAME="${STACK_NAME:-$STAGING_STACK}"
HOSTNAME_FQDN="${HOSTNAME_FQDN:-$STAGING_HOST}"
INSTANCE_TYPE="${INSTANCE_TYPE:-t4g.micro}"
HOSTED_ZONE_ID="${HOSTED_ZONE_ID:-Z039212425BG1MHVPYWDN}"
BUCKET="${BUCKET:-supervictor}"
REPO="${REPO:-JimothyJohn/supervictor}"
need aws openssl
cd "$REPO_ROOT"

if [ -z "${TAG:-}" ]; then
    need gh
    TAG="$(gh release view --repo "$REPO" --json tagName --jq .tagName)"
    [ -n "$TAG" ] || fail "no GitHub release found for $REPO — set TAG, or run release.sh first"
fi
log "release to install: $TAG"

CA_PEM="certs/ca/ca.pem"
CA_KEY="certs/ca/ca.key"
[ -f "$CA_PEM" ] || fail "missing CA cert at $CA_PEM"
[ -f "$CA_KEY" ] || fail "missing CA key at $CA_KEY"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ "${FORCE_CERT:-1}" = 0 ] && aws s3api head-object --bucket "$BUCKET" --key staging/server.pem >/dev/null 2>&1; then
    log "keeping the server certificate already in s3://$BUCKET/staging/ (FORCE_CERT=0)"
else
    log "issuing ECDSA P-256 server cert for $HOSTNAME_FQDN signed by the fleet CA"
    openssl ecparam -genkey -name prime256v1 -out "$WORK/server.key" 2>/dev/null
    openssl req -new -key "$WORK/server.key" \
        -subj "/CN=$HOSTNAME_FQDN/O=Supervictor/OU=Servers" \
        -out "$WORK/server.csr" 2>/dev/null
    openssl x509 -req -in "$WORK/server.csr" \
        -CA "$CA_PEM" -CAkey "$CA_KEY" -CAcreateserial \
        -days 365 -sha256 \
        -extfile <(printf "subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\n" "$HOSTNAME_FQDN") \
        -out "$WORK/server.pem" 2>/dev/null
    log "uploading PEMs to s3://$BUCKET/staging/"
    aws s3 cp --only-show-errors "$WORK/server.pem" "s3://$BUCKET/staging/server.pem"
    aws s3 cp --only-show-errors "$WORK/server.key" "s3://$BUCKET/staging/server.key"
fi
aws s3 cp --only-show-errors "$CA_PEM" "s3://$BUCKET/staging/ca.pem"

log "discovering default VPC + a public subnet in $REGION"
VPC_ID="$(aws ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId' --output text)"
SUBNET_ID="$(aws ec2 describe-subnets --filters Name=vpc-id,Values="$VPC_ID" Name=default-for-az,Values=true \
    --query 'Subnets[0].SubnetId' --output text)"
log "  vpc=$VPC_ID  subnet=$SUBNET_ID"

EIP_PARAMS=()
if [ -n "${EIP_ALLOCATION_ID:-}" ] || [ -n "${EIP_PUBLIC_IP:-}" ]; then
    [ -n "${EIP_ALLOCATION_ID:-}" ] && [ -n "${EIP_PUBLIC_IP:-}" ] || fail "set EIP_ALLOCATION_ID and EIP_PUBLIC_IP together"
    EIP_PARAMS=("ExistingAllocationId=$EIP_ALLOCATION_ID" "ExistingPublicIp=$EIP_PUBLIC_IP")
    log "  reusing Elastic IP $EIP_PUBLIC_IP ($EIP_ALLOCATION_ID)"
fi

log "deploying CloudFormation stack $STACK_NAME"
aws cloudformation deploy \
    --template-file supervictor/endpoint/template-staging.yaml \
    --stack-name "$STACK_NAME" \
    --capabilities CAPABILITY_IAM \
    --parameter-overrides \
        Hostname="$HOSTNAME_FQDN" \
        InstanceType="$INSTANCE_TYPE" \
        HostedZoneId="$HOSTED_ZONE_ID" \
        BundleBucket="$BUCKET" \
        Repo="$REPO" \
        ReleaseTag="$TAG" \
        VpcId="$VPC_ID" \
        SubnetId="$SUBNET_ID" \
        ${EIP_PARAMS[@]+"${EIP_PARAMS[@]}"}

log "stack outputs:"
aws cloudformation describe-stacks --stack-name "$STACK_NAME" \
    --query 'Stacks[0].Outputs[*].[OutputKey,OutputValue]' --output table

log "done. First boot installs $TAG in about a minute:"
log "  aws ssm start-session --target $(staging_instance) --region $REGION"
log "  then: sudo tail -f /var/log/bootstrap.log"
log "Pin the AMI the box came up on before any update: aws ssm put-parameter --name /supervictor/staging/ami-id --type String --overwrite --value <ami-id>"
