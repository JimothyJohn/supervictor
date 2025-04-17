#!/usr/bin/env bash

set -o errexit
set -o nounset
set -o pipefail
if [[ "${TRACE-0}" == "1" ]]; then
    set -o xtrace
fi

main() {
    source .env
    
    while [[ "$#" -gt 0 ]]; do
        case "$1" in
            -r|--release)
                RELEASE="--release"
                ;;
            -d|--desktop)
                BIN="supervictor-desktop"
                FEATURES="desktop"
                TARGET="x86_64-unknown-linux-gnu"
                BUILD_LIBS=""
                RUSTFLAGS=""
                ;;
            *)
                echo "Unknown parameter passed: $1"
                exit 1
                ;;
        esac
        shift
    done

    curl --tlsv1.2 \
        --cacert aws/AmazonRootCA1.pem  \
        --cert aws/debian.cert.pem \
        --key aws/debian.private.key \
        --request POST \
        --data "{ \"message\": \"Hello, world\" }" \
        "https://a2m9xuporrzts-ats.iot.us-east-1.amazonaws.com:8443/topics/sdk/test/python?qos=1"

}

main "$@"
