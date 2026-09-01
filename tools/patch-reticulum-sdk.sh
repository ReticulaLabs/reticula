#!/usr/bin/env bash
# Patch a vendored copy of `reticulum-sdk` so it builds for the ESP32-S3
# (xtensa-esp32s3-espidf).
#
# Why this is needed
# ------------------
# `reticulum-sdk` declares `gpio-cdev` and `tokio-serial` as unconditional
# dependencies, but they are only used by the Linux GPIO LoRa path and the
# serial interfaces respectively. On the ESP-IDF target those crates may fail
# to compile (or are useless), so this script vendors the crate and gates the
# offending modules behind features that the firmware keeps disabled.
#
# This is a stopgap until the SDK makes these dependencies optional upstream.
#
# Usage:
#   tools/patch-reticulum-sdk.sh <sdk-version> <out-dir>
#   e.g. tools/patch-reticulum-sdk.sh 2.2.30 third_party/reticulum-sdk
set -euo pipefail

SDK_VERSION="${1:-2.2.30}"
OUT_DIR="${2:-third_party/reticulum-sdk}"
REGISTRY_SRC="${CARGO_HOME:-$HOME/.cargo}/registry/src/index.crates.io-*/reticulum-sdk-${SDK_VERSION}"

mkdir -p "$OUT_DIR"

# Copy the crate from the local cargo registry if present.
shopt -s nullglob
srcs=($REGISTRY_SRC)
if [[ ${#srcs[@]} -gt 0 ]]; then
  cp -r "${srcs[0]}/." "$OUT_DIR/"
else
  echo "reticulum-sdk ${SDK_VERSION} not found in the cargo registry."
  echo "Run \`cargo fetch\` first, or pass a path to a source checkout."
  exit 1
fi

# 1. Make the LoRa GPIO module (and thus gpio-cdev) conditional.
python3 - "$OUT_DIR/Cargo.toml" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace(
    "# GPIO character device (Linux /dev/gpiochipN). v2 ABI; works on\n# mcp2221_gpio and any other modern GPIO driver.\ngpio-cdev = \"0.6\"\n",
    "# GPIO character device (Linux /dev/gpiochipN). Only needed for the\n# Linux-hosted LoRa GPIO path; kept optional for embedded targets.\ngpio-cdev = { version = \"0.6\", optional = true }\n",
)
s = s.replace("tokio-serial = \"5.4.5\"", "tokio-serial = { version = \"5.4.5\", optional = true }")
s = s.replace(
    "default = [\"alloc\"]\nalloc = []",
    "default = [\"alloc\", \"serial\"]\nalloc = []\nserial = [\"dep:tokio-serial\"]\nlora-linux-gpio = [\"dep:gpio-cdev\"]",
)
open(p, "w").write(s)
PY

# 2. Gate the imports of the Linux-only GPIO crate.
python3 - "$OUT_DIR/src/iface/lora/mod.rs" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    use gpio_cdev::{Chip, Line, LineHandle, LineRequestFlags};",
              "    #[cfg(feature = \"lora-linux-gpio\")]\n    use gpio_cdev::{Chip, Line, LineHandle, LineRequestFlags};")
open(p, "w").write(s)
PY

# 3. Gate the serial interface behind the `serial` feature.
python3 - "$OUT_DIR/src/iface.rs" "$OUT_DIR/src/lib.rs" <<'PY'
import sys
for p in sys.argv[1:]:
    s = open(p).read()
    s = s.replace("pub mod serial;", "#[cfg(feature = \"serial\")]\npub mod serial;")
    open(p, "w").write(s)
PY

echo "Patched reticulum-sdk ${SDK_VERSION} into ${OUT_DIR}"
echo
echo "To use it, add to firmware/Cargo.toml:"
echo "  [patch.crates-io]"
echo "  reticulum-sdk = { path = \"../third_party/reticulum-sdk\", default-features = false, features = [\"alloc\"] }"