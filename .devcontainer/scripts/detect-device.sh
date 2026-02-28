#!/usr/bin/env bash
# detect-device.sh — Detect the ESP32 serial device on the host and write
# ESPFLASH_PORT to .devcontainer/.device.env so the devcontainer picks it up.
#
# Called via devcontainer.json "initializeCommand" before the container starts.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.device.env"

# ANSI colours (matches quickstart/runner.py)
_BOLD="\033[1m"
_CYAN="\033[36m"
_GREEN="\033[32m"
_YELLOW="\033[33m"
_RED="\033[31m"
_RESET="\033[0m"

step()    { printf "\n${_BOLD}${_CYAN}=> %s${_RESET}\n" "$*"; }
success() { printf "${_GREEN}%s${_RESET}\n" "$*"; }
warn()    { printf "${_YELLOW}WARNING: %s${_RESET}\n" "$*" >&2; }
error()   { printf "${_RED}ERROR: %s${_RESET}\n" "$*" >&2; }

step "Detecting ESP32 serial device"

OS="$(uname -s)"

case "$OS" in
    Linux)
        DEVICE=""
        for candidate in /dev/ttyACM0 /dev/ttyACM1 /dev/ttyUSB0 /dev/ttyUSB1; do
            if [ -e "$candidate" ]; then
                DEVICE="$candidate"
                break
            fi
        done
        if [ -z "$DEVICE" ]; then
            warn "No ESP32 device found at /dev/ttyACM* or /dev/ttyUSB*."
            warn "Connect your device and rebuild the container, or set ESPFLASH_PORT manually."
            DEVICE="/dev/ttyACM0"
        fi
        ;;

    Darwin)
        # Docker Desktop on macOS does not support USB device pass-through.
        # Flash from the host using: cargo espflash flash --monitor
        # Or use web-flash inside the container over the network.
        warn "macOS host: Docker Desktop does not support USB pass-through."
        warn "Flash from the host with 'cargo espflash flash --monitor', or use web-flash."

        DEVICE=""
        for pattern in "/dev/cu.usbmodem*" "/dev/cu.SLAB_USBtoUART" "/dev/cu.wchusbserial*"; do
            # shellcheck disable=SC2086
            match="$(ls $pattern 2>/dev/null | head -1 || true)"
            if [ -n "$match" ]; then
                DEVICE="$match"
                break
            fi
        done
        DEVICE="${DEVICE:-/dev/cu.usbmodem14101}"
        ;;

    *)
        warn "Unknown OS '$OS'. Defaulting to /dev/ttyACM0."
        DEVICE="/dev/ttyACM0"
        ;;
esac

printf "ESPFLASH_PORT=%s\n" "$DEVICE" > "$ENV_FILE"
success "Serial device: $DEVICE  (written to .devcontainer/.device.env)"
