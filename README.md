<img src="assets/app.png" width="64" height="64" align="center" alt="Sony Connect Tray">

# Sony Connect Tray

A lightweight Windows system tray controller for Sony Bluetooth headphones and earbuds. Battery monitoring, ANC, Equalizer, Clear Bass, and multipoint controls in pure Rust.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4.svg)](#)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](#)
[![Size](https://img.shields.io/badge/binary-541%20KB-success.svg)](#)

---

## Features

- **Zero Bloat**: Native Win32 + Winsock2 RFCOMM. No Electron, no WebViews, ~2 MB RAM footprint.
- **Dynamic Tray Icon**: Real-time battery percentage with light/dark theme gradients, charging badge, and high-contrast outline when disconnected.
- **ANC Controls**: Toggle Noise Canceling, Off, or Ambient Sound with volume levels 1–20 and Voice Focus.
- **EQ & Clear Bass**: Instant preset switching plus dedicated Clear Bass slider (-10 to +10).
- **Multipoint Manager**: 2-device connection roster, active audio handover (🔊), and click-to-connect paired devices with auto slot eviction.
- **Native Windows 11**: DWM dark mode, Segoe UI fonts, and Fluent dialog icons.
- **Shell Integration**: Auto-recovery on Explorer restarts (`TaskbarCreated`), single-instance mutex, and optional autostart.

---

## Supported Devices

- **Over-ear**: WH-CH720N, WH-1000XM5, WH-1000XM4, WH-ULT900N, WH-XB910N, WH-CH520
- **Earbuds (TWS)**: LinkBuds S, WF-1000XM5, WF-1000XM4, LinkBuds, WF-C700N, WF-C500

---

## Usage

1. Pair your headphones in Windows under **Settings → Bluetooth & devices**.
2. Download `connect-tray.exe` from the latest [Release](https://github.com/pdimu/connect-tray/releases).
3. Run the executable. Right-click the headphone icon in the system tray to control settings.

---

## Building from Source

Prerequisites: [Rust](https://rustup.rs/) (2024 edition) and Python 3.

```powershell
# Clone the repository
git clone https://github.com/pdimu/connect-tray.git
cd connect-tray

# Build optimized binary
cargo build --release

# Stamp 10-layer PE icon and Windows 11 DPI manifest
python scripts/stamp_pe.py
```

The resulting standalone executable is at `target/release/connect-tray.exe`.

---

## Troubleshooting

- **Tray icon shows "searching...":** Make sure Bluetooth is enabled and your headphones are paired in Windows Settings. The app continuously scans and connects automatically when in range.
- **Connection takes a few seconds on wake:** Sony devices permit only one active RFCOMM control channel at a time. If the official mobile app is actively communicating, allow 2-3 seconds for handover.
- **Diagnostic logs:** To inspect connection traces, right-click the tray icon and select **other › debug logging**. Logs write to `%LOCALAPPDATA%\SonyConnectTray\trace.log` (open via **other › open logs folder**).

---

## Contributing

Contributions, bug reports, and device compatibility test results are welcome.

- **Testing new models**: If you have a Sony Bluetooth headset running MDR v2 not yet listed in the table, please open an issue or pull request with your model name and verified controls.
- **Code checks**: Run `cargo check`, `cargo test --lib`, and `cargo clippy` before submitting PRs.

---

## Authors & Credits

- **Dimu** ([@pdimu](https://github.com/pdimu)): Lead developer, UI design & implementation
- **Google Gemini 3.8 Flash**: Co-author & pair programming
- [Gadgetbridge](https://gadgetbridge.org/): Open-source wearable project documenting Sony MDR v2 RFCOMM packet structures, command payloads, and checksum algorithms
- [Plutonomicon/sony-headphones-client](https://github.com/Plutonomicon/sony-headphones-client): Desktop Sony headphone reverse-engineering research
- [Vadim-Karpenko/ear-tray](https://github.com/Vadim-Karpenko/ear-tray): Windows system tray UX and design inspiration

---

## Disclaimer

This is an unofficial, independent open-source tool. It is not affiliated with, endorsed by, or associated with Sony Group Corporation or Sony Corporation. All product names, trademarks, and registered trademarks belong to their respective owners. Protocol implementation is based on publicly available reverse-engineering research from the open-source Gadgetbridge and sony-headphones-client communities. Released for personal use.

---

## License

This project is licensed under the MIT License, see the [LICENSE](LICENSE) file for details.
