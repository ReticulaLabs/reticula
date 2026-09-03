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
#   tools/build-esp32.sh [--flash]
#
# Environment:
#   WIFI_SSID / WIFI_PASS   WiFi credentials (optional)
#   RNS_PEER                host:port Reticulum node to peer with (optional)
#   ESPFLASH_PORT           serial port for --flash (default /dev/ttyACM0)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
FIRMWARE_DIR="$ROOT/firmware"

CARGO_BIN="${CARGO_BIN:-cargo}"

# --- Verify the ESP toolchain and build prerequisites ----------------------
# The `xtensa-esp32s3-espidf` target's standard library is NOT prebuilt; it is
# compiled from source at build time with `-Zbuild-std`, which needs the
# `rust-src` component in the `esp` (nightly) toolchain. If the wrong
# toolchain is selected (e.g. the workspace's `stable`) or `rust-src` is
# missing, rustc fails with "can't find crate for `core`".
check_toolchain() {
  local tc
  tc="$(rustup toolchain list 2>/dev/null | grep -oE '^\s*esp([^\s]*)?' | tr -d ' ' | head -1 || true)"
  if [[ -z "$tc" ]]; then
    echo "ERROR: no 'esp' toolchain found." >&2
    echo "  Install it with:" >&2
    echo "    cargo install espup" >&2
    echo "    espup install" >&2
    echo "    . \$HOME/export-esp.sh" >&2
    exit 1
  fi

  # rustc must be nightly (build-std is a nightly feature).
  if ! rustup run "$tc" rustc --version 2>/dev/null | grep -qi nightly; then
    echo "ERROR: the '$tc' toolchain is not a nightly build; -Zbuild-std requires nightly." >&2
    exit 1
  fi

  # The rust-src component must be present for build-std.
  local rust_src
  rust_src="$(rustup run "$tc" rustc --print sysroot 2>/dev/null)"
  if [[ -z "$rust_src" || ! -f "$rust_src/lib/rustlib/src/rust/library/std/src/lib.rs" ]]; then
    echo "ERROR: the 'rust-src' component is missing from the '$tc' toolchain." >&2
    echo "  Fix it by re-running:" >&2
    echo "    espup install" >&2
    echo "  (it installs the rust-src component needed to build std from source)." >&2
    exit 1
  fi

  echo "==> Using toolchain: $tc (nightly, rust-src present, build-std enabled)"
}

check_toolchain

# --- Point bindgen at the esp-clang bundled with the toolchain ---------------
# esp-idf-sys regenerates its bindings with bindgen. If LIBCLANG_PATH is not
# set, bindgen falls back to the system libclang; modern clang (>= 21) breaks
# bindgen < 0.72 and produces placeholder structs (e.g. `spi_transaction_t`
# with only an `_address` field). The esp toolchain bundles a compatible clang.
if [[ -z "${LIBCLANG_PATH:-}" ]]; then
  esp_clang_lib="$(echo "$HOME/.rustup/toolchains/esp"/xtensa-esp32-elf-clang/*/esp-clang/lib 2>/dev/null | tr ' ' '\n' | head -1)"
  if [[ -n "$esp_clang_lib" && -d "$esp_clang_lib" ]]; then
    export LIBCLANG_PATH="$esp_clang_lib"
    echo "==> LIBCLANG_PATH=$LIBCLANG_PATH"
  else
    echo "WARNING: could not find the esp-clang lib dir; bindgen may use the system libclang." >&2
  fi
fi

# A previous build may have generated bindings with the wrong libclang; those
# are broken. Force esp-idf-sys to regenerate them (set RETICULA_CLEAN_BINDINGS=0
# to keep the cache).
if [[ "${RETICULA_CLEAN_BINDINGS:-1}" == "1" ]]; then
  rm -rf "$FIRMWARE_DIR/target"/xtensa-esp32s3-espidf/release/build/esp-idf-sys-* 2>/dev/null || true
  rm -rf "$FIRMWARE_DIR/target"/release/build/esp-idf-sys-* 2>/dev/null || true
fi

# `reticulum-sdk` v2.3 has the embedded support upstream; the only vendored
# fork is `embuild` (bindgen bump), wired in via `[patch.crates-io]` in
# firmware/Cargo.toml. No build-time patching is needed.
cd "$FIRMWARE_DIR"

build() {
  "$CARGO_BIN" build --release --target xtensa-esp32s3-espidf
}

flash() {
  local bin="target/xtensa-esp32s3-espidf/release/reticula-firmware"
  if [[ ! -f "$bin" ]]; then
    build
  fi
  # The T-Deck's auto-reset strap leaves the chip in download mode after a
  # DTR/RTS hard reset, so use a watchdog reset instead (boots the app). The
  # monitor connection drops after the reset; use `espflash monitor` after.
  espflash flash --after watchdog-reset --port "${ESPFLASH_PORT:-/dev/ttyACM0}" "$bin"
  echo "Flashed. Connect with: espflash monitor --port ${ESPFLASH_PORT:-/dev/ttyACM0}"
}

case "${1:-}" in
  --flash) flash ;;
  *) build ;;
esac
