#!/usr/bin/env bash
set -euo pipefail

CHIP="esp32c3"
BAUD="460800"
MONITOR_BAUD="115200"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"

BOOTLOADER="$BUILD_DIR/bootloader/bootloader.bin"
PARTITION="$BUILD_DIR/partition_table/partition-table.bin"
APP="$BUILD_DIR/supervictor.bin"

usage() {
    cat <<EOF
Usage: $(basename "$0") <command> [options]

Commands:
  build              Build firmware via Docker
  flash [PORT]       Flash firmware to device
  monitor [PORT]     Open serial monitor
  flash-monitor [PORT]  Flash then monitor
  erase [PORT]       Erase entire flash

PORT defaults to /dev/cu.usbmodem* (auto-detected) or \$SV_PORT.
EOF
    exit 1
}

detect_port() {
    if [ -n "${SV_PORT:-}" ]; then
        echo "$SV_PORT"
        return
    fi
    local port
    port=$(ls /dev/cu.usbmodem* 2>/dev/null | head -1) || true
    if [ -z "$port" ]; then
        echo "Error: no USB serial port found. Pass PORT or set SV_PORT." >&2
        exit 1
    fi
    echo "$port"
}

require_build() {
    if [ ! -f "$APP" ]; then
        echo "Error: no build found. Run '$(basename "$0") build' first." >&2
        exit 1
    fi
}

cmd_build() {
    echo "Building for $CHIP..."
    docker run --rm \
        -v "$REPO_ROOT:/project" \
        -w /project/esp-idf \
        espressif/idf \
        sh -c "idf.py set-target $CHIP && idf.py build"
    echo "Build complete: $APP"
}

cmd_flash() {
    local port="${1:-$(detect_port)}"
    require_build
    echo "Flashing to $port..."
    uv run esptool --chip "$CHIP" \
        -p "$port" \
        -b "$BAUD" \
        --before default-reset \
        --after hard-reset \
        write_flash \
        --flash_mode dio \
        --flash_size 4MB \
        --flash_freq 80m \
        0x0     "$BOOTLOADER" \
        0x8000  "$PARTITION" \
        0x10000 "$APP"
}

cmd_monitor() {
    local port="${1:-$(detect_port)}"
    echo "Monitoring $port at ${MONITOR_BAUD}baud (Ctrl-] to quit)..."
    uv run --with pyserial python -m serial.tools.miniterm "$port" "$MONITOR_BAUD" --raw
}

cmd_erase() {
    local port="${1:-$(detect_port)}"
    echo "Erasing flash on $port..."
    uv run esptool --chip "$CHIP" -p "$port" erase_flash
}

[ $# -lt 1 ] && usage

case "$1" in
    build)         cmd_build ;;
    flash)         cmd_flash "${2:-}" ;;
    monitor)       cmd_monitor "${2:-}" ;;
    flash-monitor) cmd_flash "${2:-}" && cmd_monitor "${2:-}" ;;
    erase)         cmd_erase "${2:-}" ;;
    *)             usage ;;
esac
