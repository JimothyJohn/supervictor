#!/usr/bin/env bash
# teardown-lambda-leftovers.sh — what teardown-lambda-stacks.sh left behind
# (found 2026-10-28, after the prod box took supervictor.advin.io):
#
#   1. SAM's ECR companion stacks supervictor-{dev,prod}-<hash>-CompanionStack
#      and their image repos (~500 MB of Lambda images). The repos are force-
#      deleted first: CloudFormation cannot delete a repo that holds images.
#   2. s3://supervictor/truststore.pem + truststore-dev.pem (API Gateway mTLS).
#   3. The CloudFront distribution aliased to supervictor.advin.io whose origin
#      is the deleted execute-api — outside any stack. Disabled, waited on
#      (~5-15 min), deleted. Refuses unless the origin is still execute-api.
#   4. The ACM certificate for supervictor.advin.io that distribution held,
#      once nothing uses it, and the ACM validation CNAMEs for supervictor /
#      dev.supervictor once no ACM certificate covers those names. The prod
#      box's Let's Encrypt cert is DNS-01 on _acme-challenge; untouched.
#
# Left alone: the aws-sam-cli-managed-default bucket (other projects use it),
# s3://supervictor/{prod,staging}/ (the boxes' PEMs), debian.cert.pem and
# supervictor.cert.pem in that bucket (unknown provenance — ask first).
# Needs: aws CLI with delete rights (denied to sessions — run it yourself,
# `! ./scripts/teardown-lambda-leftovers.sh` works). Idempotent: re-run after
# any failure; what is already gone is skipped.
set -euo pipefail
cd "$(dirname "$0")/.."
export AWS_DEFAULT_REGION="${AWS_REGION:-us-east-1}"
DOMAIN=supervictor.advin.io
say() { printf '[leftovers] %s\n' "$*"; }

# 1. SAM companion stacks + their ECR repos
for stack in $(aws cloudformation list-stacks --stack-status-filter CREATE_COMPLETE UPDATE_COMPLETE DELETE_FAILED \
        --query 'StackSummaries[?starts_with(StackName,`supervictor-dev-`) || starts_with(StackName,`supervictor-prod-`)].StackName' \
        --output text); do
    case "$stack" in *-CompanionStack) ;; *) say "skipping $stack (not a companion stack)"; continue ;; esac
    for repo in $(aws cloudformation describe-stack-resources --stack-name "$stack" \
            --query "StackResources[?ResourceType=='AWS::ECR::Repository'].PhysicalResourceId" --output text); do
        say "[$stack] force-deleting ECR repo $repo"
        aws ecr delete-repository --repository-name "$repo" --force >/dev/null 2>&1 || say "  (already gone)"
    done
    say "[$stack] delete-stack"
    aws cloudformation delete-stack --stack-name "$stack"
    aws cloudformation wait stack-delete-complete --stack-name "$stack" || say "  $stack did not delete cleanly — check its events"
done

# 2. API Gateway truststores
for key in truststore.pem truststore-dev.pem; do
    if aws s3api head-object --bucket supervictor --key "$key" >/dev/null 2>&1; then
        say "removing s3://supervictor/$key"; aws s3 rm "s3://supervictor/$key"
    fi
done

# 3. The orphan CloudFront distribution
dist="$(aws cloudfront list-distributions \
    --query "DistributionList.Items[?Aliases.Items && contains(Aliases.Items, '$DOMAIN')].Id" --output text)"
if [ -n "$dist" ] && [ "$dist" != None ]; then
    origin="$(aws cloudfront get-distribution --id "$dist" --query 'Distribution.DistributionConfig.Origins.Items[0].DomainName' --output text)"
    case "$origin" in *.execute-api.*) ;; *) say "ERROR: $dist origin is $origin, not execute-api — refusing"; exit 1 ;; esac
    tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
    aws cloudfront get-distribution-config --id "$dist" > "$tmp"
    etag="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["ETag"])' "$tmp")"
    enabled="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["DistributionConfig"]["Enabled"])' "$tmp")"
    if [ "$enabled" = True ]; then
        say "disabling CloudFront $dist (origin $origin)"
        python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1]))["DistributionConfig"]; d["Enabled"]=False; json.dump(d,open(sys.argv[1],"w"))' "$tmp"
        aws cloudfront update-distribution --id "$dist" --if-match "$etag" --distribution-config "file://$tmp" >/dev/null
    fi
    say "waiting for $dist to finish deploying (5-15 min)"
    aws cloudfront wait distribution-deployed --id "$dist"
    etag="$(aws cloudfront get-distribution --id "$dist" --query ETag --output text)"
    say "deleting CloudFront $dist"
    aws cloudfront delete-distribution --id "$dist" --if-match "$etag"
else
    say "no CloudFront distribution aliased to $DOMAIN"
fi

# 4. Unused ACM certs, then validation CNAMEs no certificate needs
for arn in $(aws acm list-certificates --query "CertificateSummaryList[?DomainName=='$DOMAIN' || DomainName=='dev.$DOMAIN'].CertificateArn" --output text); do
    used="$(aws acm describe-certificate --certificate-arn "$arn" --query 'length(Certificate.InUseBy)' --output text)"
    if [ "$used" = 0 ]; then say "deleting unused ACM cert $arn"; aws acm delete-certificate --certificate-arn "$arn"
    else say "ACM cert $arn still in use ($used) — kept; re-run later"; fi
done
zone="$(aws route53 list-hosted-zones --query "HostedZones[?Name=='advin.io.'].Id" --output text)"
for name in "$DOMAIN" "dev.$DOMAIN"; do
    if [ -n "$(aws acm list-certificates --query "CertificateSummaryList[?DomainName=='$name'].CertificateArn" --output text)" ]; then
        say "an ACM cert still covers $name — keeping its validation record"; continue
    fi
    # exactly one "_<token>." label above $name, so the apex pass never takes dev's record
    aws route53 list-resource-record-sets --hosted-zone-id "$zone" --output json --query ResourceRecordSets \
    | python3 -I -c '
import json, sys
name = sys.argv[1] + "."
rrs = [r for r in json.load(sys.stdin)
       if r["Type"] == "CNAME" and r["Name"].startswith("_")
       and r["Name"].split(".", 1)[1] == name
       and "acm-validations.aws" in r.get("ResourceRecords", [{}])[0].get("Value", "")]
if rrs:
    print(json.dumps({"Changes": [{"Action": "DELETE", "ResourceRecordSet": r} for r in rrs]}))
' "$name" >"${TMPDIR:-/tmp}/leftovers-dns.json"
    if [ -s "${TMPDIR:-/tmp}/leftovers-dns.json" ]; then
        say "deleting ACM validation CNAME(s) under $name"
        aws route53 change-resource-record-sets --hosted-zone-id "$zone" --change-batch "file://${TMPDIR:-/tmp}/leftovers-dns.json" >/dev/null
    fi
    rm -f "${TMPDIR:-/tmp}/leftovers-dns.json"
done

say "done. Remaining supervictor AWS footprint:"
aws cloudformation list-stacks --stack-status-filter CREATE_COMPLETE UPDATE_COMPLETE \
    --query 'StackSummaries[?contains(StackName,`supervictor`)].StackName' --output text | tr '\t' '\n' | sed 's/^/  stack /'
