#!/usr/bin/env bash

# AI-Generated comment: Enable strict error handling.
# errexit: Exit immediately if a command exits with a non-zero status.
# nounset: Treat unset variables as an error when substituting.
# pipefail: The return value of a pipeline is the status of the last command
#           to exit with a non-zero status, or zero if no command exited
#           with a non-zero status.
set -o errexit
set -o nounset
set -o pipefail

# AI-Generated comment: Enable trace mode if TRACE environment variable is set to 1.
if [[ "${TRACE-0}" == "1" ]]; then
    set -o xtrace
fi

# AI-Generated comment: Get the directory where the script resides.
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
# AI-Generated comment: Assume the project root is one level above the script directory.
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
# AI-Generated comment: Default path for the .env file.
DEFAULT_ENV_FILE="${PROJECT_ROOT}/.env"
# AI-Generated comment: Default stack name, can be overridden by .env or command-line arg.
DEFAULT_STACK_NAME="supervictor-cloud-stack"
# AI-Generated comment: Default region, can be overridden by .env or command-line arg.
DEFAULT_REGION="us-east-1"

# AI-Generated comment: Function to display help information.
usage() {
    cat << EOF
AWS SAM Deployment Helper Script

This script builds and deploys the SAM application located in the project root.
It sources deployment parameters from a .env file.

Usage:
  ./Deploy.sh [options]

Options:
  -h, --help        Show this help message
  -e, --env-file    Path to the .env file (default: ${DEFAULT_ENV_FILE})
  -s, --stack-name  CloudFormation stack name (overrides .env or default: ${DEFAULT_STACK_NAME})
  -r, --region      AWS region for deployment (overrides .env or default: ${DEFAULT_REGION})
      --no-confirm  Skip deployment confirmation prompts (uses --no-confirm-changeset)

Required .env variables:
  ApiDomainName         (e.g., api.example.com)
  AcmCertificateArn     (e.g., arn:aws:acm:...)
  TruststoreBucketName  (e.g., your-mtls-truststore-bucket)
  TruststorePrefix      (Optional, e.g., trusted-certs/)
  SamDeployBucket       (Optional, S3 bucket for SAM artifacts)

Example:
  ./Deploy.sh
  ./Deploy.sh -s my-supervictor-stack -r us-west-2
  ./Deploy.sh --env-file ../config/.env.prod --no-confirm
EOF
    exit 1
}

# AI-Generated comment: Initialize variables from defaults.
ENV_FILE="${DEFAULT_ENV_FILE}"
STACK_NAME="" # Will be set later from arg, .env, or default
REGION=""     # Will be set later from arg, .env, or default
CONFIRM_CHANGESET="true" # Deploy interactively by default

# AI-Generated comment: Parse command-line arguments.
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        -h|--help) usage ;;
        -e|--env-file) ENV_FILE="$2"; shift ;;
        -s|--stack-name) STACK_NAME="$2"; shift ;;
        -r|--region) REGION="$2"; shift ;;
        --no-confirm) CONFIRM_CHANGESET="false" ;;
        *) echo "Unknown parameter passed: $1"; usage ;;
    esac
    shift
done

# --- Environment Loading ---

# AI-Generated comment: Check if the .env file exists.
if [[ ! -f "$ENV_FILE" ]]; then
  echo "Error: Environment file not found at ${ENV_FILE}"
  usage
fi

echo "Sourcing environment variables from: ${ENV_FILE}"
# AI-Generated comment: Source the .env file, exporting variables. Using 'set -a' is safer than parsing.
set -a # Automatically export all variables defined from now on
# shellcheck source=/dev/null # Tell shellcheck to ignore inability to validate the sourced file
source "$ENV_FILE"
set +a # Stop automatically exporting variables

# AI-Generated comment: Determine final stack name and region based on priority: command-line > .env > default.
STACK_NAME="${STACK_NAME:-${STACK_NAME:-$DEFAULT_STACK_NAME}}" # Use command-line if set, else .env if set, else default
REGION="${REGION:-${AWS_REGION:-$DEFAULT_REGION}}"           # Use command-line if set, else .env (AWS_REGION common) or default

# AI-Generated comment: Validate that required variables are now set in the environment.
: "${ApiDomainName:?Error: ApiDomainName not set in environment or ${ENV_FILE}}"
: "${AcmCertificateArn:?Error: AcmCertificateArn not set in environment or ${ENV_FILE}}"
: "${TruststoreBucketName:?Error: TruststoreBucketName not set in environment or ${ENV_FILE}}"
# TruststorePrefix is optional, no check needed. Default is handled in template.
# SamDeployBucket is optional, handled below.

echo "Deployment Configuration:"
echo "  Stack Name: ${STACK_NAME}"
echo "  Region:     ${REGION}"
echo "  Env File:   ${ENV_FILE}"
echo "  API Domain: ${ApiDomainName}"
echo "  ACM ARN:    ${AcmCertificateArn}"
echo "  Truststore: s3://${TruststoreBucketName}/${TruststorePrefix:-}"
[[ -n "${SamDeployBucket:-}" ]] && echo "  SAM Bucket: ${SamDeployBucket}"
echo "  Confirm Changeset: ${CONFIRM_CHANGESET}"
echo "-------------------------------------"

# --- SAM Build ---

echo "Running sam build..."
# AI-Generated comment: Navigate to project root to run sam build/deploy.
cd "$PROJECT_ROOT"

# AI-Generated comment: Execute sam build. Use --use-container if building native extensions.
if sam build; then
  echo "SAM build completed successfully."
else
  echo "Error: SAM build failed."
  exit 1
fi

# --- SAM Deploy ---

echo "Running sam deploy..."

# AI-Generated comment: Construct the parameter overrides string dynamically.
PARAM_OVERRIDES="ParameterKey=ApiDomainName,ParameterValue=${ApiDomainName} ParameterKey=AcmCertificateArn,ParameterValue=${AcmCertificateArn} ParameterKey=TruststoreBucketName,ParameterValue=${TruststoreBucketName}"
# AI-Generated comment: Only add TruststorePrefix if it's set and not empty.
if [[ -n "${TruststorePrefix:-}" ]]; then
  PARAM_OVERRIDES="${PARAM_OVERRIDES} ParameterKey=TruststorePrefix,ParameterValue=${TruststorePrefix}"
fi

# AI-Generated comment: Base deploy command.
DEPLOY_CMD=(sam deploy \
  --stack-name "$STACK_NAME" \
  --region "$REGION" \
  --capabilities CAPABILITY_IAM CAPABILITY_AUTO_EXPAND \
  --parameter-overrides "$PARAM_OVERRIDES")

# AI-Generated comment: Add S3 bucket if specified in the environment.
if [[ -n "${SamDeployBucket:-}" ]]; then
  DEPLOY_CMD+=(--s3-bucket "$SamDeployBucket")
fi

# AI-Generated comment: Add no-confirm flag if requested.
if [[ "$CONFIRM_CHANGESET" == "false" ]]; then
  DEPLOY_CMD+=(--no-confirm-changeset)
fi

# AI-Generated comment: Execute the deploy command.
echo "Executing: ${DEPLOY_CMD[*]}"
if "${DEPLOY_CMD[@]}"; then
  echo "SAM deploy completed successfully."
else
  echo "Error: SAM deploy failed."
  exit 1
fi

echo "Deployment script finished."
exit 0
