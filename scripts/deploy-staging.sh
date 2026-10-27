#!/usr/bin/env bash
# deploy-staging.sh — CREATE a box for a stage: one EC2 instance that installs a
# GitHub release of the single-binary endpoint on first boot.
#
#   TAG=v0.2.0 ./scripts/deploy-staging.sh               # the staging box
#   STAGE=prod TAG=v0.2.0 ./scripts/deploy-staging.sh    # the prod box (supervictor.advin.io)
#
# What it does:
#   1. issues an ECDSA P-256 server certificate for the hostname from the fleet
#      CA (certs/ca/ca.key, kept on this machine only) — unless FORCE_CERT=0 and
#      one is already in S3; staging-letsencrypt.sh swaps in a public one later
#   2. uploads ca.pem / server.pem / server.key to s3://$BUCKET/$STAGE/
#   3. creates supervictor/endpoint/template-staging.yaml as supervictor-endpoint-$STAGE;
#      the instance's bootstrap fetches the PEMs and runs scripts/install-endpoint.sh $TAG
#   4. pins the AMI the box came up on at /supervictor/$STAGE/ami-id, which
#      staging-stack-update.sh passes so no later update replaces the instance
#
# Creation only: an existing stack is refused, because a plain deploy
# re-resolves the "latest AL2023" AMI alias and REPLACES the instance (SQLite
# file, Let's Encrypt cert, hand-installed build gone). Changes to a live
# stack go through scripts/staging-stack-update.sh. The prod box needs
# supervictor.advin.io free first (scratch/01-teardown-lambda-stacks.sh): the
# stack creates the A record and CloudFormation refuses a name another stack owns.
#
#   STAGE             staging (default) | prod — stack name, hostname, S3 prefix, AMI pin
#   TAG               release the box installs (default: the newest GitHub release)
#   STACK_NAME        default supervictor-endpoint-$STAGE
#   HOSTNAME_FQDN     default staging.supervictor.advin.io / supervictor.advin.io
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
if aws cloudformation describe-stacks --stack-name "$STACK_NAME" >/dev/null 2>&1; then
    fail "stack $STACK_NAME already exists — this script only creates. Update it with: STAGE=$STAGE ./scripts/staging-stack-update.sh [Key=Value ...]"
fi
log "creating $STACK_NAME for $HOSTNAME_FQDN (stage=$STAGE type=$INSTANCE_TYPE PEMs under s3://$BUCKET/$S3_PREFIX/)"

CA_PEM="certs/ca/ca.pem"
CA_KEY="certs/ca/ca.key"
[ -f "$CA_PEM" ] || fail "missing CA cert at $CA_PEM"
[ -f "$CA_KEY" ] || fail "missing CA key at $CA_KEY"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ "${FORCE_CERT:-1}" = 0 ] && aws s3api head-object --bucket "$BUCKET" --key "$S3_PREFIX/server.pem" >/dev/null 2>&1; then
    log "keeping the server certificate already in s3://$BUCKET/$S3_PREFIX/ (FORCE_CERT=0)"
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
    log "uploading PEMs to s3://$BUCKET/$S3_PREFIX/"
    aws s3 cp --only-show-errors "$WORK/server.pem" "s3://$BUCKET/$S3_PREFIX/server.pem"
    aws s3 cp --only-show-errors "$WORK/server.key" "s3://$BUCKET/$S3_PREFIX/server.key"
fi
aws s3 cp --only-show-errors "$CA_PEM" "s3://$BUCKET/$S3_PREFIX/ca.pem"

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
        Stage="$STAGE" \
        Hostname="$HOSTNAME_FQDN" \
        InstanceType="$INSTANCE_TYPE" \
        HostedZoneId="$HOSTED_ZONE_ID" \
        BundleBucket="$BUCKET" \
        CaCertKey="$S3_PREFIX/ca.pem" \
        ServerCertKey="$S3_PREFIX/server.pem" \
        ServerKeyKey="$S3_PREFIX/server.key" \
        Repo="$REPO" \
        ReleaseTag="$TAG" \
        VpcId="$VPC_ID" \
        SubnetId="$SUBNET_ID" \
        ${EIP_PARAMS[@]+"${EIP_PARAMS[@]}"}

INSTANCE_ID="$(stack_resource "$STACK_NAME" Instance)"
AMI="$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" --query 'Reservations[0].Instances[0].ImageId' --output text)"
log "pinning AMI $AMI at $AMI_PARAM (staging-stack-update.sh passes it so no update replaces the box)"
aws ssm put-parameter --name "$AMI_PARAM" --type String --value "$AMI" --overwrite >/dev/null

log "stack outputs:"
aws cloudformation describe-stacks --stack-name "$STACK_NAME" \
    --query 'Stacks[0].Outputs[*].[OutputKey,OutputValue]' --output table

log "done. First boot installs $TAG in about a minute:"
log "  aws ssm start-session --target $INSTANCE_ID --region $REGION"
log "  then: sudo tail -f /var/log/bootstrap.log"
log "then: STAGE=$STAGE ./scripts/staging-letsencrypt.sh for a publicly trusted certificate"
