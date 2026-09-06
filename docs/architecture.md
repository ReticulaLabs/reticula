# Reticula architecture

This document explains how Reticula is put together and the reasoning behind
the major design decisions.

## Goals

1. An **end-client** for the Reticulum mesh running on small devices such as
   the ESP32-S3 T-Deck.
2. A **critical, keyboard-driven UI** for interacting with the mesh.
3. **Modular hardware support** — one application, many devices.
4. Two MVP applications: **LXMF chat** and a **NomadNet browser**.

## Layering

```
                reticula-sim (host)          reticula-firmware (ESP32-S3)
                   |                                |
             reticula-host BSP                  reticula-tdeck BSP
                   \                                /
                    \                              /
                    reticula-app  (board + UI + network + view model)
                     /           \
              reticula-ui     reticula-lxmf   reticula-nomad
                 |                |               |
              reticula-hal   \    |    reticulum-sdk (transport)
                   \          \   |   /
                    BSPs       app-layer clients
```

Each crate has one responsibility:

| Crate            | Responsibility                                                        |
|------------------|------------------------------------------------------------------------|
| `reticula-hal`   | `Display`, `Keyboard`, `Board` traits + logical `KeyCode`s. `no_std`.  |
| `reticula-lxmf`  | LXMF message wire format + `LxmfClient` (deliver/receive over links).  |
| `reticula-nomad` | Micron page parser + `NomadClient` (node discovery + page fetch).      |
| `reticula-ui`    | `embedded-graphics` widget toolkit + the set of screens.               |
| `reticula-app`   | Owns the `Transport`, clients, view model and the UI event loop.       |
| `reticula-host`  | Terminal simulator: RGB565 framebuffer rendered as colour ASCII art.   |
| `reticula-tdeck` | T-Deck: ST7789 over SPI (`mipidsi`), keyboard over I²C (`0x55`).      |

`reticula-sim` and `reticula-firmware` are thin binaries: they pick a board,
an identity and a network configuration, then call `ReticulaApp::new().run()`.

## The hardware abstraction

`reticula-hal::Board` is the only thing a BSP must implement:

```rust
pub trait Board {
    type Display: reticula_hal::Display;
    type Keyboard: Keyboard;
    fn display(&mut self) -> Option<&mut Self::Display>;
    fn keyboard(&mut self) -> &mut Self::Keyboard;
    fn delay_ms(&mut self, ms: u32);
    fn uptime_ms(&self) -> u64;
}
```

`Display` exposes a concrete `embedded-graphics` `DrawTarget<Color = Rgb565>`.
Because `DrawTarget` is not object-safe, the UI is **generic over the target**
and monomorphised per board — the standard embedded-graphics pattern. This is
why the whole app is compiled once per device.

`Keyboard` is a non-blocking poll interface (`read(&mut [KeyEvent]) -> usize`),
which works both in a background-thread-fed desktop queue and on a polled I²C
bus. `KeyCode` is a logical key enum; BSPs map their raw bytes onto it.

## Reticulum integration

`reticulum-app::build_transport` creates a `Transport` with end-client-only
settings:

* `retransmit(false)` — never forward/relay for others;
* `reroute_eager(false)`, no discovery, no blackhole;
* outbound connectivity only: a UDP interface (bind + optional forward peer)
  or a TCP client interface.

The LXMF **delivery identity** is registered as the `lxmf/delivery`
destination and announced periodically, so peers can find a path back to the
device. NomadNet nodes are discovered by watching announces and matching the
`nomadnetwork/node` destination hash computed from the announced identity.

### LXMF

The wire format (`reticula-lxmf::message`) matches the reference
implementation exactly:

```
packed = destination_hash(16) ‖ source_hash(16)
       ‖ ed25519_signature(64)
       ‖ msgpack([timestamp, title, content, fields])
hash   = SHA256(destination ‖ source ‖ packed_payload)
sign   = ed25519(source_key, hashed_part ‖ hash)
```

Delivery for the MVP is **DIRECT**: the client packs the message, establishes
(or reuses) an encrypted link to the recipient and sends the packed bytes.
Inbound messages arrive as link data and are validated against the source
identity recalled from announces. The `MessageStore` is bounded (512 messages)
and groups messages per peer for the conversation list.

### NomadNet

A browser fetches pages over links using Reticulum request/response:
`transport.link_request(link_id, "/page/index.mu", Nil)`, waiting for the
matching `LinkResponse`. Pages are Micron markup; `reticula-nomad::page`
renders the subset relevant to a small screen (headings, emphasis, links,
comments) into display lines plus extractable `rns://` links for navigation.

## The UI and event flow

Screens (`reticula-ui::screens::Screen`) are a fixed enum, each holding only
its own ephemeral UI state (selection, scroll, composer buffer). Keys are
mapped by the screen to a `Command`; the app executes commands asynchronously:

```
keyboard → Screen::handle_key → Command::SendMessage/FetchPage/... 
                                     ↓ (tokio::spawn)
                        LxmfClient::send / NomadClient::fetch_page
                                     ↓ (broadcast events)
                     app updates SharedState view model
                                     ↓
                         Screen::render(ctx) → flush
```

The render loop and the network tasks communicate only through the small
`SharedState` (std mutexes, never held across `await`), which keeps the
synchronous render path cheap and simple. Page fetches are spawned off the UI
loop; results land in shared state and the page view redraws when ready.

## The desktop simulator

`reticula-host` renders the 320×240 RGB565 framebuffer to the terminal as
colour ASCII art (each cell ≈ one 6×10 glyph). A background `crossterm` reader
feeds the keyboard queue. This lets the full application — real Reticulum over
UDP, real LXMF — run on a desktop with zero hardware.

## Building for the ESP32-S3

The firmware uses the **ESP-IDF (std) framework** (`xtensa-esp32s3-espidf`)
because `reticulum-sdk` is built on tokio and needs a std runtime. This gives
us:

* tokio on the device for the transport;
* `esp-idf-svc` for WiFi;
* `esp-idf-hal` for SPI (LCD), I²C (keyboard) and GPIO;
* `mipidsi` for the ST7789.

### Toolchain

```bash
cargo install espup && espup install          # installs `esp` toolchain + target
. "$HOME/export-esp.sh"
tools/build-esp32.sh                          # builds firmware/
tools/build-esp32.sh --flash                  # builds + flashes via espflash
```

Firmware configuration is supplied at build time:

| Variable    | Meaning                                          |
|-------------|--------------------------------------------------|
| `WIFI_SSID` | Station SSID to connect to (optional).           |
| `WIFI_PASS` | Station password.                                |
| `RNS_PEER`  | `host:port` Reticulum node to peer with (optional). |

WiFi credentials and the Reticulum identity can also be changed at runtime from
the Settings menu (Identity / WiFi sub-menus). Changes are persisted to NVS
(namespace `reticula`, keys `identity`, `wifi_ssid`, `wifi_pass`) and applied on
the next boot: the device restarts, then reads NVS credentials first (falling
back to the build-time `WIFI_SSID`/`WIFI_PASS`).

### Embedded support

`reticulum-sdk` v2.3 gained the embedded/ESP32 support upstream: the
serial/LoRa interfaces (`serial`, `rnode`, `kiss`, `lora`) are feature-gated
so their `tokio-serial`/`serialport` and Linux-only `gpio-cdev` dependencies
drop out of embedded builds; `AtomicU64` uses `portable-atomic` (no hardware
64-bit CAS on the ESP32-S3); the tokio feature set is slimmed; and the LoRa
interface supports an `embedded-hal` SPI + GPIO backend. The project uses it
from crates.io with `default-features = false`.

Two vendored forks are wired in via `[patch.crates-io]`:

* `reticulum-sdk` (`third_party/`): runs the LoRa chipset work (open/init, RX
  IRQ polling, transmit) on the tokio worker instead of `spawn_blocking`. On
  the ESP32, blocking-pool threads need a large OS stack allocated from
  internal RAM (ESP-IDF pthreads are hardcoded to `MALLOC_CAP_INTERNAL`), which
  fails with ENOMEM once WiFi/TCP/worker stacks are in use.
* `embuild` (`third_party/`): bumps `bindgen` to 0.72 — bindgen < 0.72.1 emits
  broken bindings (`_address` placeholder structs) with clang ≥ 21, which
  breaks `esp-idf-sys` bindings on systems with a modern system libclang.

`tools/build-esp32.sh` also points `bindgen` at the `esp-clang` bundled with
the toolchain (`LIBCLANG_PATH`) as a further safeguard.

### LoRa on the T-Deck

The T-Deck's SX1262 (CS=9, BUSY=13, RST=17, DIO1=45) shares the SPI2 bus with
the LCD. `reticula-tdeck` exposes the radio through `TDeckBoard::lora_hw()` (an
`embedded-hal`-based `LoRaHwProvider`); `reticula-app`'s `lora` feature spawns
a `LoRaInterface<SX1262>` when `NetConfig::lora` is set. The firmware enables
it at build time with `LORA_FREQ=<hz>`, or at runtime from Settings → LoRa
(frequency kHz, bandwidth kHz, spreading factor, coding rate, TX power, and an
enable/disable toggle), persisted to NVS and applied on the next boot.

### Memory

The T-Deck has 8 MB of PSRAM and 512 KB of on-chip SRAM. The firmware
`profile.release` is tuned for size (`opt-level = "s"`, `lto`, `panic = "abort"`).
Large allocations (the LXMF SPI buffer, page data, store) should live in
PSRAM; the `sdkconfig.defaults` enables `SPIRAM_USE_MALLOC`.

### Runtime bring-up on the ESP32-S3

Getting the SDK to run on-device took several ESP-IDF-specific fixes:

* **OPI PSRAM**: the T-Deck's 8 MB PSRAM is octal, so `sdkconfig.defaults`
  sets `CONFIG_SPIRAM_MODE_OCT=y` + `CONFIG_SPIRAM_SPEED_80M=y`. Without it,
  `cpu_start` aborts with "Failed to init external RAM!".
* **Console**: `CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG=y` routes `log` output to
  the native USB-Serial-JTAG (this T-Deck has no separate USB-UART bridge).
* **`eventfd` VFS**: the tokio IO driver calls `eventfd()`, which ESP-IDF
  provides via a VFS that must be registered first. `main()` calls
  `esp_vfs_eventfd_register()` before building the runtime, otherwise the
  build fails with `EACCES`.
* **lwIP without WiFi**: `main()` calls `esp_netif_init()` unconditionally.
  Otherwise the UDP interface hits `tcpip_send_msg_wait_sem: Invalid mbox`
  because the tcpip thread is only started by WiFi init.
* **Stacks**: `CONFIG_ESP_MAIN_TASK_STACK_SIZE=65536` for the `app_main`
  task (the SDK init overflows 32 KB) and `thread_stack_size(65536)` for the
  tokio worker threads (the transport task overflows the 8 KB pthread
  default).
* **Broadcast channels**: `TransportConfig::set_event_channel_capacity(512)`.
  The SDK default of 16384 pre-allocates ~8.6 MB *per channel* (7 channels),
  which exceeds the 8 MB PSRAM.
* **Resetting to boot**: the T-Deck's auto-reset strap leaves the chip in
  download mode after a DTR/RTS reset, so `tools/build-esp32.sh` flashes with
  `--after watchdog-reset` (the app then boots normally).
* **Peripheral power rail**: GPIO10 (`BOARD_POWERON`) powers the LCD, LoRa
  radio, SD card and keyboard and must be driven HIGH — without it the display
  stays black. `esp-idf-hal`'s `PinDriver` also resets a pin on drop
  (`gpio_reset_without_pull`), so both GPIO10 and the backlight (GPIO42) are
  stored in `TDeckBoard` and kept alive for the board's lifetime (a local pin
  driver would turn them off at the end of `new()`).
* **No-flicker rendering**: `mipidsi` draws straight to the SPI bus, so
  clearing + redrawing every frame visibly blinks the panel. `TdeckScreen`
  renders into an offscreen RGB565 framebuffer (in PSRAM) and only pushes it
  to the panel when the frame actually changed — a static screen is written
  once and stays put.

## Roadmap notes

* **Persistence** — `reticula-app::identity::load_or_create(None)` generates a
  fresh identity per boot on the device today. Wiring NVS/SPIFFS storage means
  passing a storage backend to that function and to `MessageStore`.
* **Opportunistic / store-and-forward** — the flat-encryption helpers
  (`message::encrypt_for`/`decrypt_with`) and the opportunistic receive path
  are already in place; store-and-forward needs a propagation-node client.
* **Trackball / touch** — the T-Deck trackball I²C sensor is stubbed
  (`reticula-tdeck::trackball`); a pointer device can be added to
  `reticula-hal::Board` without touching the UI.