# <img src="assets/app.png" width="36" height="36" align="center" alt="Sony Connect Tray"> Sony Connect Tray

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
