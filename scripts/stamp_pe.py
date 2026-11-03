"""
Stamps multi-resolution PE icon resources and Windows 11 PerMonitorV2 manifest
into the compiled connect-tray.exe binary using Win32 UpdateResourceW.
"""

import argparse
import ctypes
from ctypes import wintypes
import os
import struct
import sys

def stamp(exe_path: str, ico_path: str):
    if not os.path.exists(exe_path):
        print(f"error: executable not found at '{exe_path}'")
        print("hint: run 'cargo build --release' first")
        sys.exit(1)

    if not os.path.exists(ico_path):
        print(f"error: icon not found at '{ico_path}'")
        sys.exit(1)

    kernel32 = ctypes.windll.kernel32

    BeginUpdateResourceW = kernel32.BeginUpdateResourceW
    BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]
    BeginUpdateResourceW.restype = wintypes.HANDLE

    UpdateResourceW = kernel32.UpdateResourceW
    UpdateResourceW.argtypes = [
        wintypes.HANDLE,
        wintypes.LPCWSTR,
        wintypes.LPCWSTR,
        wintypes.WORD,
        wintypes.LPVOID,
        wintypes.DWORD,
    ]
    UpdateResourceW.restype = wintypes.BOOL

    EndUpdateResourceW = kernel32.EndUpdateResourceW
    EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]
    EndUpdateResourceW.restype = wintypes.BOOL

    with open(ico_path, "rb") as f:
        ico_bytes = f.read()

    reserved, img_type, img_count = struct.unpack("<HHH", ico_bytes[:6])

    grp_header = struct.pack("<HHH", 0, img_type, img_count)
    grp_entries = []
    icons = []
    offset = 6

    for i in range(1, img_count + 1):
        w, h, colors, res, planes, bpp, size, img_off = struct.unpack(
            "<BBBBHHII", ico_bytes[offset : offset + 16]
        )
        grp_entries.append(
            struct.pack("<BBBBHHIH", w, h, colors, res, planes, bpp, size, i)
        )
        icons.append((i, ico_bytes[img_off : img_off + size]))
        offset += 16

    grp_data = grp_header + b"".join(grp_entries)

    RT_ICON = ctypes.cast(3, wintypes.LPCWSTR)
    RT_GROUP_ICON = ctypes.cast(14, wintypes.LPCWSTR)
    RT_MANIFEST = ctypes.cast(24, wintypes.LPCWSTR)

    h_update = BeginUpdateResourceW(exe_path, False)
    if not h_update:
        err = ctypes.GetLastError()
        raise RuntimeError(f"BeginUpdateResourceW failed with error {err}")

    for icon_id, icon_data in icons:
        buf = (ctypes.c_char * len(icon_data)).from_buffer_copy(icon_data)
        res_id = ctypes.cast(icon_id, wintypes.LPCWSTR)
        if not UpdateResourceW(h_update, RT_ICON, res_id, 1033, buf, len(icon_data)):
            EndUpdateResourceW(h_update, True)
            raise RuntimeError(f"UpdateResourceW RT_ICON failed: {ctypes.GetLastError()}")

    grp_buf = (ctypes.c_char * len(grp_data)).from_buffer_copy(grp_data)
    if not UpdateResourceW(
        h_update, RT_GROUP_ICON, ctypes.cast(1, wintypes.LPCWSTR), 1033, grp_buf, len(grp_data)
    ):
        EndUpdateResourceW(h_update, True)
        raise RuntimeError(f"UpdateResourceW RT_GROUP_ICON failed: {ctypes.GetLastError()}")

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
    if not UpdateResourceW(
        h_update, RT_MANIFEST, ctypes.cast(1, wintypes.LPCWSTR), 1033, man_buf, len(manifest_xml)
    ):
        EndUpdateResourceW(h_update, True)
        raise RuntimeError(f"UpdateResourceW RT_MANIFEST failed: {ctypes.GetLastError()}")

    if not EndUpdateResourceW(h_update, False):
        raise RuntimeError(f"EndUpdateResourceW failed: {ctypes.GetLastError()}")

    print(f"stamped {img_count}-layer PE icon and PerMonitorV2 manifest into {exe_path}")

def main():
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    default_exe = os.path.join(repo_root, "target", "release", "connect-tray.exe")
    default_ico = os.path.join(repo_root, "assets", "app.ico")

    parser = argparse.ArgumentParser(description="Stamp PE icon and manifest into connect-tray.exe")
    parser.add_argument("--exe", default=default_exe, help="Path to connect-tray.exe")
    parser.add_argument("--ico", default=default_ico, help="Path to app.ico")
    args = parser.parse_args()

    stamp(args.exe, args.ico)

if __name__ == "__main__":
    main()
