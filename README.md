# <img src="assets/app.png" width="36" height="36" align="center" alt="Sony Connect Tray"> Sony Connect Tray

A lightweight Windows system tray controller for Sony Bluetooth headphones and earbuds. Battery monitoring, ANC, Equalizer, Clear Bass, and multipoint controls in pure Rust. One tiny standalone executable.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0078d4.svg)](#)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](#)
[![Size](https://img.shields.io/badge/binary-~550%20KB-success.svg)](#)

---

## Highlights

- **Zero Electron / WebView bloat**: written in pure Rust using native Win32 APIs and Winsock2 RFCOMM. Cold starts instantly and idles at under 2 MB of RAM.
- **Direct Sony MDR v2 protocol**: communicates directly with the headphone firmware over Bluetooth Serial Port Profile (SPP / RFCOMM) without relying on third-party daemons or bridge services.
- **Single portable binary**: no installer required. Run `connect-tray.exe` from anywhere, with optional one-click Start Menu shortcut and autostart registration.
- **Native Windows 11 UI**: dark and light theme awareness, DWM immersive dark mode, dynamic system typography, and modern Fluent vector icons.

---

## Features

- **Dynamic Tray Icon & Live Tooltips**:
  - Live battery percentage rendered directly onto the system tray icon with multi-tier vertical gradients matching Windows dark and light taskbar themes.
  - Displays a charging lightning badge during active charging.
  - High-contrast monochrome outline icon when disconnected that remains visible on both black and white taskbars.
  - Hovering over the icon displays active device model, connection state, and battery status.
- **Active Noise Cancellation (ANC)**:
  - Toggle between **Noise Canceling**, **Ambient Sound**, and **Off**.
  - Progressive disclosure for Ambient Sound: adjust ambient volume levels (1–20) and toggle **Voice Focus** when active.
- **Equalizer & Clear Bass**:
  - Instant EQ preset switching (*Off, Bright, Excited, Mellow, Relaxed, Vocal, Treble Boost, Bass Boost, Speech, Manual, Custom 1, Custom 2*).
  - Fine-grained **Clear Bass** control with dedicated slider adjustments from `-10` to `+10`.
- **Multipoint Connection Manager**:
  - Visual roster of connected host devices mirroring the official mobile app.
  - Real-time active audio source indicator (`🔊`).
  - One-click audio playback handover between your Windows PC and your phone or tablet.
  - Click-to-connect paired devices with automatic non-PC slot eviction when both multipoint slots are occupied.
- **Sound Quality Mode**:
  - Switch between **Priority on Sound Quality** (LDAC) and **Priority on Stable Connection**.
- **Hardware Settings**:
  - Toggle **Voice Guidance** prompts on or off.
  - Enable or disable the headphone **Touch Sensor Control Panel**.
  - Configure **Automatic Power Off** idle timeouts.
- **Shell Integration & Resilience**:
  - Listens for Windows Explorer restarts via `TaskbarCreated` to automatically restore the tray icon if `explorer.exe` crashes.
  - Single-instance named mutex (`Local\SonyConnectTrayInstanceMutex`) with automatic window wake-up and stale instance takeover.
  - Built-in diagnostics dialog (`About`) with live session packet statistics, uptime counter, and clipboard export.
  - Clean one-click uninstaller that removes HKCU Run autostart keys, Start Menu shortcuts, and local application cache.

---

## Supported Devices

`connect-tray` supports Sony headphones and earbuds running the **Sony MDR v2** RFCOMM protocol:

| Category | Model | Tested Features |
| :--- | :--- | :--- |
| **Over-Ear** | WH-CH720N | Battery, ANC (Levels 1–20 + Voice Focus), EQ, Clear Bass, Multipoint |
| **Over-Ear** | WH-1000XM5 | Battery, ANC, Ambient Sound, EQ, Clear Bass, Multipoint |
| **Over-Ear** | WH-1000XM4 | Battery, ANC, Ambient Sound, EQ, Clear Bass, Multipoint |
| **Over-Ear** | WH-ULT900N (ULT WEAR) | Battery, ANC, EQ, Multipoint |
| **Over-Ear** | WH-XB910N | Battery, ANC, EQ, Clear Bass, Multipoint |
| **Over-Ear** | WH-CH520 | Battery, EQ, Multipoint |
| **In-Ear (TWS)** | LinkBuds S (WF-LS900N) | Multi-battery (L/R/Case), ANC, EQ, Multipoint |
| **In-Ear (TWS)** | WF-1000XM5 | Multi-battery (L/R/Case), ANC, EQ, Multipoint |
| **In-Ear (TWS)** | WF-1000XM4 | Multi-battery (L/R/Case), ANC, EQ, Multipoint |
| **In-Ear (TWS)** | LinkBuds (WF-L900) | Multi-battery (L/R/Case), EQ, Multipoint |
| **In-Ear (TWS)** | WF-C700N / WF-C500 | Multi-battery, ANC, EQ |

---

## Usage

1. Pair your Sony headphones or earbuds in Windows under **Settings → Bluetooth & devices**.
2. Download the latest `connect-tray.exe` from the [Releases](https://github.com/pdimu/connect-tray/releases) page.
3. Run `connect-tray.exe`.
4. Right-click the headphone icon in the taskbar notification area to open the control menu.
5. (Optional) Check **run on start-up** in the menu to automatically copy the binary to `%LOCALAPPDATA%\SonyConnectTray` and register a Start Menu shortcut and Windows startup entry.

---

## Troubleshooting

- **Tray icon shows "searching...":**
  Ensure your headphones are powered on and paired with Windows. The app continuously scans for paired Sony Bluetooth devices in the background and will automatically connect once detected.
- **Connection takes a few seconds after wake:**
  Sony headphones only allow one active RFCOMM management session at a time. If the official mobile app is currently connected and actively transmitting, it may take 2–3 seconds for the headphone firmware to open the channel for Windows.
- **Diagnostic Logging:**
  To inspect Bluetooth packets and connection states, right-click the tray icon and select **other › debug logging**. Logs are stored at `%LOCALAPPDATA%\SonyConnectTray\trace.log` (accessible directly via **other › open logs folder**).

---

## Building from Source

### Prerequisites
- [Rust](https://rustup.rs/) (2024 edition, stable toolchain)
- Windows 10 or 11 (64-bit)
- Python 3 (for embedding the multi-resolution PE icon and application manifest)

```powershell
# Clone the repository
git clone https://github.com/pdimu/connect-tray.git
cd connect-tray

# Build optimized release binary
cargo build --release

# Stamp 10-layer PE icon resource and PerMonitorV2 manifest
python -c '
import struct, ctypes
from ctypes import wintypes

kernel32 = ctypes.windll.kernel32
BeginUpdateResourceW = kernel32.BeginUpdateResourceW
BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]
BeginUpdateResourceW.restype = wintypes.HANDLE
UpdateResourceW = kernel32.UpdateResourceW
UpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.WORD, wintypes.LPVOID, wintypes.DWORD]
UpdateResourceW.restype = wintypes.BOOL
EndUpdateResourceW = kernel32.EndUpdateResourceW
EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]
EndUpdateResourceW.restype = wintypes.BOOL

exe_path = r"target\release\connect-tray.exe"
ico_path = r"assets\app.ico"

with open(ico_path, "rb") as f:
    ico_bytes = f.read()

reserved, img_type, img_count = struct.unpack("<HHH", ico_bytes[:6])
grp_header = struct.pack("<HHH", 0, img_type, img_count)
grp_entries = []
icons = []
offset = 6
for i in range(1, img_count + 1):
    w, h, colors, res, planes, bpp, size, img_off = struct.unpack("<BBBBHHII", ico_bytes[offset:offset+16])
    grp_entries.append(struct.pack("<BBBBHHIH", w, h, colors, res, planes, bpp, size, i))
    icons.append((i, ico_bytes[img_off:img_off+size]))
    offset += 16

grp_data = grp_header + b"".join(grp_entries)
h_update = BeginUpdateResourceW(exe_path, False)

for icon_id, icon_data in icons:
    buf = (ctypes.c_char * len(icon_data)).from_buffer_copy(icon_data)
    UpdateResourceW(h_update, ctypes.cast(3, wintypes.LPCWSTR), ctypes.cast(icon_id, wintypes.LPCWSTR), 1033, buf, len(icon_data))

grp_buf = (ctypes.c_char * len(grp_data)).from_buffer_copy(grp_data)
UpdateResourceW(h_update, ctypes.cast(14, wintypes.LPCWSTR), ctypes.cast(1, wintypes.LPCWSTR), 1033, grp_buf, len(grp_data))

manifest_xml = b"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.32.0.0" processorArchitecture="*" name="SonyConnectTray" type="win32"/>
  <description>Sony Connect Tray</description>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>"""

man_buf = (ctypes.c_char * len(manifest_xml)).from_buffer_copy(manifest_xml)
UpdateResourceW(h_update, ctypes.cast(24, wintypes.LPCWSTR), ctypes.cast(1, wintypes.LPCWSTR), 1033, man_buf, len(manifest_xml))
EndUpdateResourceW(h_update, False)
print("Build complete: target/release/connect-tray.exe stamped successfully")
'
```

---

## Authors & Credits

- **Dimu** ([@pdimu](https://github.com/pdimu)) — Lead developer, reverse-engineering & UI design
- **Google Gemini 3.8 Flash** — Co-author & pair programming
- Inspired by [Vadim-Karpenko/ear-tray](https://github.com/Vadim-Karpenko/ear-tray) for Nothing / CMF hardware.

---

## License

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
