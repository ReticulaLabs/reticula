#!/usr/bin/env bash
# Build the Reticula firmware for the ESP32-S3 (LILYGO T-Deck).
#
# Requirements:
#   - Rust `esp` toolchain + `xtensa-esp32s3-espidf` target (install via espup)
#   - ESP-IDF exported into the environment
#
#   cargo install espup
#   espup install
#   . $HOME/export-esp.sh
#
# Usage:
#   tools/build-esp32.sh [--flash] [WIFI_SSID] [WIFI_PASS] [RNS_PEER]
#
# Example:
#   WIFI_SSID=MyNet WIFI_PASS=secret RNS_PEER=192.168.1.10:5238 \
#     tools/build-esp32.sh --flash
set -euo pipefail

cd "$(dirname "$0")/../firmware"

CARGO_BIN="${CARGO_BIN:-cargo}"

build() {
  "$CARGO_BIN" build --release --target xtensa-esp32s3-espidf
}

flash() {
  local bin="target/xtensa-esp32s3-espidf/release/reticula-firmware"
  if [[ ! -f "$bin" ]]; then
    build
  fi
  espflash flash --monitor --port "${ESPFLASH_PORT:-/dev/ttyACM0}" "$bin"
}

case "${1:-}" in
  --flash) flash ;;
  *) build ;;
esac