# Reticula

An embedded **end-client** for the [Reticulum](https://reticulum.network/)
mesh network, written in Rust. It targets small handheld devices such as the
[LILYGO T-Deck](https://lilygo.cc/products/t-deck) (ESP32-S3) and brings an
**LXMF chat client** and a **NomadNet browser** to the mesh — with a keyboard
and LCD-driven interface.

Reticula is deliberately a *client*: it never forwards traffic for others, so
it is a good citizen on a low-memory device and cheap on the mesh.

## Features (MVP)

* **LXMF chat** — wire-compatible with the reference LXMF implementation
  (same packed message format, hashing and Ed25519 signatures). Send and
  receive messages over encrypted Reticulum links.
* **NomadNet browser** — discover `nomadnetwork/node` destinations, fetch
  pages over links and render Micron markup (headings, emphasis, links) on
  screen.
* **Modular hardware abstraction** — display, keyboard and board are traits in
  `reticula-hal`; any device is a new BSP crate. Two ship today:
  * `reticula-host` — a **desktop terminal simulator** so the whole UI can be
    developed without hardware;
  * `reticula-tdeck` — the ESP32-S3 T-Deck (ST7789 LCD over SPI, keyboard
    over I²C).
* **Identity persistence** on the host; wire-up points documented for flash
  storage on the device.

## Quick start (desktop simulator)

```bash
cargo build --release -p reticula-sim

# join a local Reticulum mesh over UDP
cargo run -p reticula-sim -- --udp-bind 0.0.0.0:5238

# peer with a specific node
cargo run -p reticula-sim -- --udp-bind 0.0.0.0:5238 --udp-forward 192.168.1.10:5238
```

Controls: arrows/Enter to navigate, type in chat, `Esc` to go back (and exit
on the home screen). The identity is stored in `~/.config/reticula/identity.key`.

## Building for the T-Deck

The firmware uses the ESP-IDF (std) framework, since `reticulum-sdk` runs on
tokio. Install the toolchain, then:

```bash
cargo install espup && espup install && . "$HOME/export-esp.sh"

WIFI_SSID=MyNet WIFI_PASS=secret RNS_PEER=192.168.1.10:5238 \
  tools/build-esp32.sh --flash
```

See [docs/architecture.md](docs/architecture.md) for details on the ESP32
toolchain and a small `reticulum-sdk` packaging note.

## Repository layout

```
crates/
├── reticula-hal/       # display / keyboard / board abstraction traits
├── reticula-lxmf/      # LXMF wire format + client (over reticulum-sdk)
├── reticula-nomad/     # NomadNet page parser + browser client
├── reticula-ui/        # embedded-graphics UI: widgets + screens
├── reticula-app/       # application: wires board + UI + network + state
├── reticula-host/      # desktop terminal simulator BSP
└── reticula-tdeck/     # LILYGO T-Deck BSP (ESP32-S3)
sim/                    # `reticula` desktop simulator binary
firmware/               # `reticula-firmware` ESP32 binary
tools/                  # build + packaging helpers
```

## Design notes

* **End-client only.** The transport is created with `retransmit=false` and no
  discovery/blackhole roles; the device announces only its own LXMF delivery
  identity and connects out to the mesh over UDP or TCP.
* **Memory-conscious.** Messages are kept in a bounded in-memory store,
  rendering reads a per-frame snapshot, and screens share state rather than
  duplicating it.
* **Protocol fidelity.** LXMF and NomadNet are implemented against the
  reference wire formats so Reticula interoperates with the Python ecosystem.

## Roadmap

- [x] LXMF chat over direct links
- [x] NomadNet page browsing (fetch + render + follow links)
- [ ] Identity + message persistence on the device (NVS / SPIFFS)
- [ ] Trackball / touch input for the T-Deck
- [ ] Opportunistic (single-packet) and store-and-forward delivery
- [ ] LXMF receipts (read/delivered), typing indicators
- [ ] LoRa interface (SX1262) on the T-Deck

## License

MIT — see [LICENSE](LICENSE).