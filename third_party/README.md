# Vendored forks

Reticula builds against one vendored fork, wired in via `[patch.crates-io]` in
`firmware/Cargo.toml` and excluded from the main workspace. The change is
small, self-contained and intended to be submitted upstream.

> `reticulum-sdk` is **not** vendored any more: v2.3.0 gained the embedded
> support upstream (feature-gated serial/LoRa interfaces, `portable-atomic`
> 64-bit counters, a slimmed tokio feature set, and an embedded-hal LoRa
> backend). The project uses it directly from crates.io.

## `embuild` (`embuild/`)

Clone of <https://github.com/esp-rs/embuild> (v0.33.4) with one commit on top:
"Bump bindgen to 0.72". bindgen < 0.72.1 produces broken bindings — structs
reduced to a 1-byte `_address` placeholder — when parsing with clang >= 21
(e.g. a system clang 22). `esp-idf-sys` 0.36 uses embuild's bindgen, so this
bump is required for the ESP-IDF bindings to generate correctly.