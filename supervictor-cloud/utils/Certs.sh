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

# AI-Generated comment: Function to display help information.
usage() {
    cat << EOF
AWS Certificate Generation and API Gateway Truststore Upload Utility

Usage:
  ./Certs.sh -n <base_name> -b <s3_bucket> [options]

Required Arguments:
  -n, --name        Base name for the generated files (e.g., mydevice)
  -b, --bucket      S3 bucket name used as the API Gateway truststore

Options:
  -h, --help        Show this help message
  -d, --days        Optional. Validity duration in days for the certificate (default: 365)
  -s, --subj        Optional. Subject line for the certificate (default: "/CN=<base_name>")
  -p, --prefix      Optional. S3 prefix (folder) within the bucket to upload the certificate to

Examples:
  ./Certs.sh --name my-iot-device --bucket my-truststore-bucket
  ./Certs.sh -n test-cert -b my-truststore-bucket -p trusted-certs/ -d 730
  ./Certs.sh -n dev1 -b api-mtls-certs -s "/C=US/O=MyOrg/CN=dev1.example.com"
EOF
    exit 1
}

# AI-Generated comment: Initialize variables with default values.
NAME=""
DAYS=365
SUBJ=""
S3_BUCKET=""
S3_PREFIX=""

# AI-Generated comment: Parse command-line arguments.
# This loop processes arguments until none are left ($# is greater than 0).
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        -h|--help)
            # AI-Generated comment: Display help and exit if -h or --help is provided.
            usage
            ;;
        -n|--name)
            # AI-Generated comment: Capture the base name provided after -n or --name.
            # shift moves to the next argument ($2 becomes $1).
            NAME="$2"
            shift
            ;;
        -d|--days)
            # AI-Generated comment: Capture the validity days provided after -d or --days.
            DAYS="$2"
            shift
            ;;
        -s|--subj)
            # AI-Generated comment: Capture the subject line provided after -s or --subj.
            SUBJ="$2"
            shift
            ;;
        -b|--bucket)
            # AI-Generated comment: Capture the S3 bucket name provided after -b or --bucket.
            S3_BUCKET="$2"
            shift
            ;;
        -p|--prefix)
            # AI-Generated comment: Ensure prefix ends with a slash if provided.
            S3_PREFIX="${2%/}/" # Remove trailing slash if exists, then add one.
            shift
            ;;
        *)
            # AI-Generated comment: Handle unknown parameters.
            echo "Unknown parameter passed: $1"
            usage
            ;;
    esac
    # AI-Generated comment: Move to the next argument pair or single argument.
    shift
done

# AI-Generated comment: Validate required arguments.
# Check if the NAME variable is empty (-z). If so, display an error and usage info.
if [[ -z "$NAME" ]]; then
    echo "Error: Base name (-n or --name) is required."
    usage
fi
if [[ -z "$S3_BUCKET" ]]; then
    echo "Error: S3 bucket name (-b or --bucket) is required."
    usage
fi

# AI-Generated comment: Set default subject if not provided.
# If SUBJ variable is empty, construct a default Common Name (CN) using the base name.
if [[ -z "$SUBJ" ]]; then
    SUBJ="/CN=$NAME"
fi

# AI-Generated comment: Define output filenames based on the provided name.
PRIVATE_KEY_FILE="${NAME}.key.pem"
CERT_FILE="${NAME}.cert.pem"
PUBLIC_KEY_FILE="${NAME}.pub.pem"

# AI-Generated comment: Check if AWS CLI is installed and accessible.
if ! command -v aws &> /dev/null; then
    echo "Error: AWS CLI command ('aws') not found. Please install and configure it."
    exit 1
fi

# AI-Generated comment: Generate the private key using openssl.
# genpkey: Generates a private key.
# -algorithm RSA: Specifies the RSA algorithm. Other options like ED25519 could be used.
# -out: Specifies the output file for the private key.
echo "Generating private key: ${PRIVATE_KEY_FILE}"
openssl genpkey -algorithm RSA -out "$PRIVATE_KEY_FILE"

# AI-Generated comment: Generate a self-signed X.509 certificate.
# req: Certificate Signing Request (CSR) and certificate generation utility.
# -new: Creates a new certificate request (or self-signed cert with -x509).
# -x509: Outputs a self-signed certificate instead of a CSR.
# -key: Specifies the private key to use for signing.
# -out: Specifies the output file for the certificate.
# -days: Sets the validity period of the certificate.
# -subj: Sets the subject name directly, avoiding interactive prompts.
echo "Generating self-signed certificate: ${CERT_FILE}"
openssl req -new -x509 -key "$PRIVATE_KEY_FILE" -out "$CERT_FILE" -days "$DAYS" -subj "$SUBJ"

# AI-Generated comment: Extract the public key from the certificate.
# x509: Certificate display and signing utility.
# -pubkey: Outputs the public key.
# -noout: Prevents outputting the encoded version of the certificate itself.
# -in: Specifies the input certificate file.
# The output is redirected (>) to the public key file.
echo "Extracting public key: ${PUBLIC_KEY_FILE}"
openssl x509 -pubkey -noout -in "$CERT_FILE" > "$PUBLIC_KEY_FILE"

# AI-Generated comment: Define the target S3 path.
S3_TARGET_PATH="s3://${S3_BUCKET}/${S3_PREFIX}${CERT_FILE}"

# AI-Generated comment: Upload the certificate to the S3 truststore bucket.
echo "Uploading certificate ${CERT_FILE} to S3 truststore: ${S3_TARGET_PATH}"
if aws s3 cp "$CERT_FILE" "$S3_TARGET_PATH"; then
    echo "Successfully uploaded certificate to S3."
else
    echo "Error: Failed to upload certificate to S3. Check AWS CLI configuration and permissions."
    # AI-Generated comment: Optionally clean up generated files on failure? Decided against it for easier debugging.
    exit 1
fi

# AI-Generated comment: Print confirmation message with generated filenames and S3 location.
echo ""
echo "Successfully generated and uploaded:"
echo "  Private Key: ${PRIVATE_KEY_FILE} (Keep this secure on the device!)"
echo "  Certificate: ${CERT_FILE}"
echo "  Public Key:  ${PUBLIC_KEY_FILE}"
echo "  Uploaded To: ${S3_TARGET_PATH}"
echo ""
echo "Reminder: Ensure your API Gateway custom domain name is configured to use s3://${S3_BUCKET}/${S3_PREFIX} as its truststore."
echo "Authorization for the Lambda endpoint is managed via API Gateway settings and IAM, not directly by this certificate."

exit 0
