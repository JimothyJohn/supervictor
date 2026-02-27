#!/usr/bin/env bash
# gen_certs.sh — Generate mTLS certificates for Supervictor
#
# Usage:
#   ./scripts/gen_certs.sh ca                      Initialize the root CA
#   ./scripts/gen_certs.sh device <name> [days]    Issue a device certificate
#   ./scripts/gen_certs.sh person <name> [days]    Issue a person certificate
#   ./scripts/gen_certs.sh list                    List all issued certificates
#
# Outputs:
#   certs/ca/ca.pem               CA certificate — upload to s3://supervictor/truststore.pem
#   certs/ca/ca.key               CA private key  — keep secret, never commit
#   certs/devices/<name>/client.pem
#   certs/devices/<name>/client.key
#   certs/people/<name>/client.pem
#   certs/people/<name>/client.key
#
# Examples:
#   ./scripts/gen_certs.sh ca
#   ./scripts/gen_certs.sh device factory-floor-01
#   ./scripts/gen_certs.sh person alice --days 90
#   ./scripts/gen_certs.sh list

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERTS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/certs"
CA_DIR="$CERTS_DIR/ca"

DAYS_CA=3650
DAYS_CLIENT=365

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

check_openssl() {
    if ! command -v openssl &>/dev/null; then
        echo "Error: openssl not found on PATH." >&2
        echo "Install it via: brew install openssl  (macOS) or apt install openssl  (Debian/Ubuntu)" >&2
        exit 1
    fi
}

usage() {
    grep '^#' "$0" | sed 's/^# \{0,1\}//' | head -20
    exit 1
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

cmd_ca() {
    local days="${1:-$DAYS_CA}"
    mkdir -p "$CA_DIR"

    if [[ -f "$CA_DIR/ca.key" ]]; then
        echo "Error: CA already exists at $CA_DIR" >&2
        echo "Delete certs/ca/ and re-run to regenerate (WARNING: revokes all issued certs)." >&2
        exit 1
    fi

    echo "Generating root CA..."
    openssl genrsa -out "$CA_DIR/ca.key" 4096 2>/dev/null
    openssl req -new -x509 -days "$days" \
        -key "$CA_DIR/ca.key" \
        -out "$CA_DIR/ca.pem" \
        -subj "/CN=SupervictorCA/O=Supervictor" \
        2>/dev/null

    chmod 600 "$CA_DIR/ca.key"
    chmod 644 "$CA_DIR/ca.pem"

    echo ""
    echo "Root CA created."
    echo "  Certificate : $CA_DIR/ca.pem"
    echo "  Private key : $CA_DIR/ca.key  (keep secret)"
    echo "  Valid for   : $days days"
    echo ""
    echo "Next step — upload to S3 truststore:"
    echo "  aws s3 cp $CA_DIR/ca.pem s3://supervictor/truststore.pem"
}

cmd_issue() {
    local entity_type="$1"   # device | person
    local name="$2"
    local days="${3:-$DAYS_CLIENT}"

    local ou
    case "$entity_type" in
        device) ou="Devices" ;;
        person) ou="People"  ;;
        *) echo "Error: unknown type '$entity_type' (use device or person)" >&2; exit 1 ;;
    esac

    if [[ ! -f "$CA_DIR/ca.key" ]]; then
        echo "Error: CA not found — run first: ./scripts/gen_certs.sh ca" >&2
        exit 1
    fi

    local subdir
    case "$entity_type" in
        device) subdir="devices" ;;
        person) subdir="people"  ;;
    esac

    local out_dir="$CERTS_DIR/$subdir/$name"
    mkdir -p "$out_dir"

    if [[ -f "$out_dir/client.key" ]]; then
        echo "Error: Certificate for '$name' already exists at $out_dir" >&2
        echo "Delete the directory and re-run to reissue." >&2
        exit 1
    fi

    echo "Issuing $entity_type certificate for '$name'..."
    openssl genrsa -out "$out_dir/client.key" 2048 2>/dev/null
    openssl req -new \
        -key "$out_dir/client.key" \
        -out "$out_dir/client.csr" \
        -subj "/CN=${name}/O=Supervictor/OU=${ou}" \
        2>/dev/null
    openssl x509 -req -days "$days" \
        -in "$out_dir/client.csr" \
        -CA "$CA_DIR/ca.pem" \
        -CAkey "$CA_DIR/ca.key" \
        -CAcreateserial \
        -out "$out_dir/client.pem" \
        2>/dev/null
    rm "$out_dir/client.csr"

    chmod 600 "$out_dir/client.key"
    chmod 644 "$out_dir/client.pem"

    echo ""
    echo "Certificate issued."
    echo "  Type        : $entity_type"
    echo "  Subject DN  : CN=${name},O=Supervictor,OU=${ou}"
    echo "  Certificate : $out_dir/client.pem"
    echo "  Private key : $out_dir/client.key"
    echo "  Valid for   : $days days"
    echo ""
    echo "To use with curl:"
    echo "  curl --cert $out_dir/client.pem --key $out_dir/client.key https://supervictor.advin.io/hello"
}

cmd_list() {
    if [[ ! -d "$CERTS_DIR" ]]; then
        echo "No certs directory found at $CERTS_DIR"
        exit 0
    fi

    local found=0
    for type_dir in "$CERTS_DIR"/devices "$CERTS_DIR"/people; do
        [[ -d "$type_dir" ]] || continue
        local entity_type
        entity_type="$(basename "$type_dir")"

        for cert in "$type_dir"/*/client.pem; do
            [[ -f "$cert" ]] || continue
            found=1
            local name
            name="$(basename "$(dirname "$cert")")"
            local subject not_before not_after
            subject=$(openssl x509 -noout -subject -in "$cert" 2>/dev/null | sed 's/subject=//')
            not_before=$(openssl x509 -noout -startdate -in "$cert" 2>/dev/null | sed 's/notBefore=//')
            not_after=$(openssl x509 -noout -enddate -in "$cert" 2>/dev/null | sed 's/notAfter=//')
            printf "  %-10s %-20s %s  →  %s\n" "$entity_type" "$name" "$not_before" "$not_after"
        done
    done

    if [[ $found -eq 0 ]]; then
        echo "No certificates issued yet."
        echo "Run: ./scripts/gen_certs.sh ca  then  ./scripts/gen_certs.sh device <name>"
    fi
}

# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

check_openssl

case "${1:-}" in
    ca)
        cmd_ca "${2:-}"
        ;;
    device|person)
        [[ -n "${2:-}" ]] || { echo "Usage: gen_certs.sh $1 <name> [days]"; exit 1; }
        cmd_issue "$1" "$2" "${3:-}"
        ;;
    list)
        cmd_list
        ;;
    -h|--help|help|"")
        usage
        ;;
    *)
        echo "Error: unknown command '$1'" >&2
        usage
        ;;
esac
