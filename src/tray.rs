#![allow(unsafe_op_in_unsafe_fn, static_mut_refs)]

use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetSysColor,
    GetSysColorBrush, LineTo, MoveToEx, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, UpdateWindow, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBITMAP,
    HBRUSH, HDC, HFONT, LOGFONTW, PAINTSTRUCT,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryA};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, DrawIconEx, EndMenu, GetClientRect, GetCursorPos,
    GetMenuItemID, GetMessageW, GetSystemMetrics, GetWindowTextW, IsChild, IsWindow, LoadCursorW,
    LoadIconW, NONCLIENTMETRICSW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    SystemParametersInfoW, TrackPopupMenuEx, TranslateMessage, BS_OWNERDRAW, HICON, HMENU,
    IDC_ARROW, MF_CHECKED, MF_DISABLED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETNONCLIENTMETRICS, SW_SHOWNORMAL, TPM_RECURSE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MENURBUTTONUP, WM_NULL, WM_PAINT, WM_RBUTTONUP,
    WM_SETFONT, WM_SETTINGCHANGE, WM_USER, WNDCLASSW, WS_CHILD, WS_EX_DLGMODALFRAME,
    WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

#[link(name = "user32")]
unsafe extern "system" {
    fn EnableWindow(hwnd: HWND, b_enable: i32) -> i32;
}

use crate::icon::{destroy_icon_safe, render_tray_icon};
use crate::protocol::{AncMode, ConnectionMode, EqPreset, APO_NAMES};
use crate::state::{BatteryTopology, DeviceManager};

type HRESULT = i32;

const CF_UNICODETEXT: u32 = 13;
const ES_READONLY: u32 = 0x0800;
const ES_AUTOHSCROLL: u32 = 0x0080;
const SS_LEFT: u32 = 0x0000;
const WS_CAPTION: u32 = 0x00C00000;
const WS_SYSMENU: u32 = 0x00080000;
const WM_SETICON: u32 = 0x0080;
const WM_CTLCOLORBTN: u32 = 0x0135;
const VK_ESCAPE: u32 = 0x1B;
const PS_SOLID: i32 = 0;
const COLOR_BTNFACE: i32 = 15;
const COLOR_WINDOWTEXT: i32 = 8;

pub const APP_TITLE_VERSION: &str = concat!(
    "Sony Connect Tray v",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

#[inline]
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

type FnSetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FnAllowDarkModeForWindow = unsafe extern "system" fn(HWND, i32) -> i32;
type FnFlushMenuThemes = unsafe extern "system" fn();
type FnSetWindowTheme = unsafe extern "system" fn(HWND, *const u16, *const u16) -> HRESULT;
type FnDwmSetWindowAttribute = unsafe extern "system" fn(HWND, u32, *const std::ffi::c_void, u32) -> HRESULT;
type FnLoadIconWithScaleDown = unsafe extern "system" fn(
    hinst: *mut std::ffi::c_void,
    psz_name: *const u16,
    cx: i32,
    cy: i32,
    phico: *mut HICON,
) -> HRESULT;

static mut FN_FLUSH_MENU_THEMES: Option<FnFlushMenuThemes> = None;
static mut FN_SET_WINDOW_THEME: Option<FnSetWindowTheme> = None;
static mut FN_ALLOW_DARK_MODE_WINDOW: Option<FnAllowDarkModeForWindow> = None;
static mut FN_DWM_SET_WINDOW_ATTRIBUTE: Option<FnDwmSetWindowAttribute> = None;

unsafe fn load_modern_system_icon(icon_id: *const u16, cx: i32, cy: i32) -> HICON {
    let comctl32 = LoadLibraryA(b"comctl32.dll\0".as_ptr());
    if !comctl32.is_null() {
        let func: Option<FnLoadIconWithScaleDown> =
            std::mem::transmute(GetProcAddress(comctl32, b"LoadIconWithScaleDown\0".as_ptr()));
        if let Some(load_scaled) = func {
            let mut hico: HICON = null_mut();
            if load_scaled(null_mut(), icon_id, cx, cy, &mut hico) == 0 && !hico.is_null() {
                return hico;
            }
        }
    }
    LoadIconW(null_mut(), icon_id)
}

pub unsafe fn init_dark_mode_support() {
    let uxtheme = LoadLibraryA(b"uxtheme.dll\0".as_ptr());
    if !uxtheme.is_null() {
        let fn_set_preferred: Option<FnSetPreferredAppMode> =
            std::mem::transmute(GetProcAddress(uxtheme, 135 as usize as *const u8));
        if let Some(set_preferred) = fn_set_preferred {
            set_preferred(1); // AllowDark
        }

        FN_ALLOW_DARK_MODE_WINDOW =
            std::mem::transmute(GetProcAddress(uxtheme, 133 as usize as *const u8));
        FN_FLUSH_MENU_THEMES =
            std::mem::transmute(GetProcAddress(uxtheme, 136 as usize as *const u8));
        FN_SET_WINDOW_THEME =
            std::mem::transmute(GetProcAddress(uxtheme, b"SetWindowTheme\0".as_ptr()));
    }

    let dwmapi = LoadLibraryA(b"dwmapi.dll\0".as_ptr());
    if !dwmapi.is_null() {
        FN_DWM_SET_WINDOW_ATTRIBUTE =
            std::mem::transmute(GetProcAddress(dwmapi, b"DwmSetWindowAttribute\0".as_ptr()));
    }
}

pub unsafe fn apply_window_dark_mode(hwnd: HWND) {
    if let Some(allow_window) = FN_ALLOW_DARK_MODE_WINDOW {
        allow_window(hwnd, 1);
    }
    if let Some(set_theme) = FN_SET_WINDOW_THEME {
        let class = to_wide("DarkMode_Explorer");
        set_theme(hwnd, class.as_ptr(), null_mut());
    }
}

pub unsafe fn flush_menu_themes() {
    if let Some(flush) = FN_FLUSH_MENU_THEMES {
        flush();
    }
}

pub unsafe fn set_dwm_dark_mode(hwnd: HWND, enable: bool) {
    if let Some(dwm_func) = FN_DWM_SET_WINDOW_ATTRIBUTE {
        let val: i32 = if enable { 1 } else { 0 };
        let _ = dwm_func(hwnd, 20 /* DWMWA_USE_IMMERSIVE_DARK_MODE */, &val as *const _ as *const _, 4);
        let _ = dwm_func(hwnd, 19 /* DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1 */, &val as *const _ as *const _, 4);
    }
}

pub unsafe fn is_system_dark_mode() -> bool {
    let mut hkey: HKEY = null_mut();
    let subkey = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) == 0 {
        let mut val: u32 = 1;
        let mut size = 4u32;
        let val_name = to_wide("AppsUseLightTheme");
        let res = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            null_mut(),
            &mut val as *mut u32 as *mut u8,
            &mut size,
        );
        RegCloseKey(hkey);
        if res == 0 {
            return val == 0;
        }
    }
    false
}

pub unsafe fn get_system_ui_fonts() -> (HFONT, HFONT) {
    let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
    ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;

    let res = SystemParametersInfoW(
        SPI_GETNONCLIENTMETRICS,
        ncm.cbSize,
        &mut ncm as *mut _ as *mut std::ffi::c_void,
        0,
    );

    if res != 0 {
        let regular_font = CreateFontIndirectW(&ncm.lfMessageFont);
        let mut bold_lf = ncm.lfMessageFont;
        bold_lf.lfWeight = 700; // FW_BOLD
        let bold_font = CreateFontIndirectW(&bold_lf);
        (regular_font, bold_font)
    } else {
        let mut lf: LOGFONTW = std::mem::zeroed();
        lf.lfHeight = -12;
        lf.lfWeight = 400;
        let face = to_wide("Segoe UI");
        let len = face.len().min(lf.lfFaceName.len() - 1);
        lf.lfFaceName[..len].copy_from_slice(&face[..len]);
        let regular_font = CreateFontIndirectW(&lf);
        lf.lfWeight = 700;
        let bold_font = CreateFontIndirectW(&lf);
        (regular_font, bold_font)
    }
}

pub unsafe fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    if OpenClipboard(hwnd) == 0 {
        return false;
    }
    EmptyClipboard();
    let wide = to_wide(text);
    let byte_len = wide.len() * 2;
    let h_mem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
    if !h_mem.is_null() {
        let ptr = GlobalLock(h_mem) as *mut u16;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            GlobalUnlock(h_mem);
            SetClipboardData(CF_UNICODETEXT, h_mem as _);
        }
    }
    CloseClipboard();
    true
}

pub const WM_TRAY_CALLBACK: u32 = WM_USER + 1;
pub const WM_STATE_UPDATED: u32 = WM_APP + 1;
pub const WM_RESTORE_TRAY: u32 = WM_USER + 2;

static mut WM_TASKBAR_CREATED: u32 = 0;

// Menu Command IDs
const ID_HEADER_STATUS: usize = 1000;
const ID_HEADER_DEV: usize = 1001;
const ID_BAT_LEFT: usize = 1002;
const ID_BAT_RIGHT: usize = 1003;
const ID_BAT_CASE: usize = 1004;
const ID_BAT_SINGLE: usize = 1005;
const ID_FW_VER: usize = 1006;
const ID_CODEC: usize = 1007;

const ID_ANC_NC: usize = 1010;
const ID_ANC_ASM: usize = 1011;
const ID_ANC_OFF: usize = 1012;
const ID_ASM_VOICE_FOCUS: usize = 1013;
const ID_ASM_LEVEL_20: usize = 1014;
const ID_CONN_SOUND_QUALITY: usize = 1015;
const ID_CONN_STABLE: usize = 1016;
const ID_ASM_LEVEL_15: usize = 1017;
const ID_ASM_LEVEL_10: usize = 1018;
const ID_ASM_LEVEL_5: usize = 1019;

const ID_EQ_OFF: usize = 1020;
const ID_EQ_BASS_BOOST: usize = 1021;
const ID_EQ_TREBLE_BOOST: usize = 1022;
const ID_EQ_VOCAL: usize = 1023;
const ID_EQ_SPEECH: usize = 1024;
const ID_EQ_RELAXED: usize = 1025;
const ID_EQ_BRIGHT: usize = 1026;
const ID_EQ_MANUAL: usize = 1027;
const ID_ASM_LEVEL_1: usize = 1028;

const ID_DSEE_TOGGLE: usize = 1030;
const ID_WEARING_SENSOR_TOGGLE: usize = 1031;
const ID_SPEAK_TO_CHAT_TOGGLE: usize = 1032;
const ID_MULTIPOINT_TOGGLE: usize = 1033;
const ID_VOICE_GUIDANCE_TOGGLE: usize = 1034;
const ID_TOUCH_SENSOR_TOGGLE: usize = 1035;
const ID_MULTIPOINT_SWITCH_THIS_PC: usize = 1036;
const ID_MULTIPOINT_SWITCH_OTHER: usize = 1037;

const ID_APO_0: usize = 1040;
const ID_APO_1: usize = 1041;
const ID_APO_2: usize = 1042;
const ID_APO_3: usize = 1043;
const ID_APO_4: usize = 1044;
const ID_APO_5: usize = 1045;

const ID_CLEAR_BASS_BASE: usize = 1050; // 1050..=1070 for values -10..=+10

const ID_REFRESH: usize = 1090;
const ID_STARTUP_TOGGLE: usize = 1091;
const ID_EXIT: usize = 1099;
const ID_MULTIPOINT_DEV_BASE: usize = 1100; // 1100..=1131 for clickable multipoint device slots
const ID_POPUP_PIN_FOCUS: usize = 1140;
const ID_POPUP_DISCONNECT: usize = 1141;
const ID_HEADSET_BASE: usize = 1150; // 1150..=1170 for switching paired Sony headsets

const ID_ABOUT: usize = 2001;
const ID_SOURCE_CODE: usize = 2002;
const ID_DEBUG_TOGGLE: usize = 2003;
const ID_OPEN_LOGS: usize = 2004;
const ID_RESTART: usize = 2005;
const ID_UNINSTALL: usize = 2006;

use std::sync::atomic::{AtomicBool, Ordering};
static DEBUG_LOGGING: AtomicBool = AtomicBool::new(false);

pub fn is_debug_logging() -> bool {
    DEBUG_LOGGING.load(Ordering::Relaxed) || std::env::var_os("CONNECT_TRAY_DEBUG").is_some()
}

pub fn toggle_debug_logging() -> bool {
    let prev = DEBUG_LOGGING.load(Ordering::Relaxed);
    DEBUG_LOGGING.store(!prev, Ordering::Relaxed);
    !prev
}

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_REG_NAME: &str = "SonyConnectTray";

static mut GLOBAL_MANAGER: Option<Arc<DeviceManager>> = None;
static mut GLOBAL_HWND: HWND = null_mut();
static mut PREV_ICON: HICON = null_mut();
static mut LAST_CONNECTED: bool = false;

pub fn get_app_data_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("SonyConnectTray");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn get_start_menu_shortcut_path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| {
        std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Sony Connect Tray.lnk")
    })
}

pub fn is_run_on_startup() -> bool {
    let mut hkey: HKEY = null_mut();
    let subkey = to_wide(RUN_KEY_PATH);
    let val_name = to_wide(APP_REG_NAME);

    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return false;
        }

        let mut data_type = 0u32;
        let mut data_len = 0u32;
        let res = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            &mut data_type,
            null_mut(),
            &mut data_len,
        );
        RegCloseKey(hkey);
        res == 0 && data_len > 0
    }
}

pub fn toggle_run_on_startup() -> bool {
    let current = is_run_on_startup();
    let mut hkey: HKEY = null_mut();
    let subkey = to_wide(RUN_KEY_PATH);
    let val_name = to_wide(APP_REG_NAME);

    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_WRITE, &mut hkey) != 0 {
            return current;
        }

        if current {
            let _ = RegDeleteValueW(hkey, val_name.as_ptr());
            RegCloseKey(hkey);
            if let Some(lnk) = get_start_menu_shortcut_path() {
                let _ = std::fs::remove_file(lnk);
            }
            false
        } else {
            let app_dir = get_app_data_dir();
            let target_exe = app_dir.join("connect-tray.exe");

            if let Ok(cur_exe) = std::env::current_exe() {
                if cur_exe != target_exe {
                    let _ = std::fs::copy(&cur_exe, &target_exe);
                }
            }

            let quoted_target = format!("\"{}\"", target_exe.display());
            let wide_path = to_wide(&quoted_target);
            let byte_len = (wide_path.len() * 2) as u32;
            let _ = RegSetValueExW(
                hkey,
                val_name.as_ptr(),
                0,
                REG_SZ,
                wide_path.as_ptr() as *const u8,
                byte_len,
            );
            RegCloseKey(hkey);

            if let Some(lnk_path) = get_start_menu_shortcut_path() {
                if !lnk_path.exists() {
                    use std::os::windows::process::CommandExt;
                    let ps = format!(
                        "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{},0'; $s.Save()",
                        lnk_path.display(),
                        target_exe.display(),
                        target_exe.display()
                    );
                    let _ = std::process::Command::new("powershell")
                        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
                        .creation_flags(0x08000000)
                        .output();
                }
            }

            true
        }
    }
}

pub fn create_fallback_headphone_icon() -> HICON {
    #[rustfmt::skip]
    let and_mask: [u8; 32] = [
        0b11111111, 0b11111111,
        0b11110000, 0b00001111,
        0b11100000, 0b00000111,
        0b11001111, 0b11110011,
        0b11001111, 0b11110011,
        0b11001111, 0b11110011,
        0b10001111, 0b11110001,
        0b10001111, 0b11110001,
        0b10001111, 0b11110001,
        0b10001111, 0b11110001,
        0b10001111, 0b11110001,
        0b11001111, 0b11110011,
        0b11111111, 0b11111111,
        0b11111111, 0b11111111,
        0b11111111, 0b11111111,
        0b11111111, 0b11111111,
    ];

    #[rustfmt::skip]
    let xor_mask: [u8; 32] = [
        0b00000000, 0b00000000,
        0b00001111, 0b11110000,
        0b00011111, 0b11111000,
        0b00110000, 0b00001100,
        0b00110000, 0b00001100,
        0b00110000, 0b00001100,
        0b01110000, 0b00001110,
        0b01110000, 0b00001110,
        0b01110000, 0b00001110,
        0b01110000, 0b00001110,
        0b01110000, 0b00001110,
        0b00110000, 0b00001100,
        0b00000000, 0b00000000,
        0b00000000, 0b00000000,
        0b00000000, 0b00000000,
        0b00000000, 0b00000000,
    ];

    unsafe {
        let h_inst = GetModuleHandleW(null_mut());
        CreateIcon(
            h_inst,
            16,
            16,
            1,
            1,
            and_mask.as_ptr(),
            xor_mask.as_ptr(),
        )
    }
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            null_mut(),
            to_wide("open").as_ptr(),
            to_wide(url).as_ptr(),
            null_mut(),
            null_mut(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn log_trace(msg: &str) {
    if !cfg!(debug_assertions) && !is_debug_logging() {
        return;
    }
    use std::io::Write;
    let log_path = get_app_data_dir().join("trace.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(
            f,
            "[{}] {msg}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
    }
}

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    log_trace(&format!(
        "window_proc msg: 0x{:04x}, wparam: 0x{:x}, lparam: 0x{:x}",
        msg, wparam, lparam
    ));
    match msg {
        msg if msg == WM_TASKBAR_CREATED && WM_TASKBAR_CREATED != 0 => {
            log_trace("WM_TASKBAR_CREATED received from Explorer, restoring tray icon");
            force_readd_tray_icon(hwnd);
            0
        }
        WM_RESTORE_TRAY => {
            log_trace("WM_RESTORE_TRAY received from duplicate instance");
            force_readd_tray_icon(hwnd);
            show_tray_menu(hwnd);
            0
        }
        WM_TRAY_CALLBACK => {
            let event = lparam as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_tray_menu(hwnd);
            }
            0
        }
        WM_STATE_UPDATED => {
            if let Some(ref manager) = GLOBAL_MANAGER {
                let connected = {
                    let s = manager.state.lock().unwrap();
                    s.connected
                };
                if connected != LAST_CONNECTED {
                    LAST_CONNECTED = connected;
                    EndMenu();
                }
            }
            update_tray_icon(hwnd);
            0
        }
        WM_SETTINGCHANGE => {
            log_trace("WM_SETTINGCHANGE received, flushing menu themes and updating tray icon");
            flush_menu_themes();
            update_tray_icon(hwnd);
            0
        }
        WM_COMMAND => {
            let cmd_id = wparam as usize;
            handle_menu_command(hwnd, cmd_id);
            0
        }
        WM_MENURBUTTONUP => {
            let item_pos = wparam as i32;
            let h_menu = lparam as HMENU;
            let item_id = GetMenuItemID(h_menu, item_pos) as usize;
            log_trace(&format!("WM_MENURBUTTONUP: item_pos={}, item_id={}", item_pos, item_id));
            if (ID_MULTIPOINT_DEV_BASE..ID_MULTIPOINT_DEV_BASE + 32).contains(&item_id) {
                let dev_idx = item_id - ID_MULTIPOINT_DEV_BASE;
                if let Some(ref manager) = GLOBAL_MANAGER {
                    let (target_dev, is_pinned) = {
                        let s = manager.state.lock().unwrap();
                        let d = s.capabilities
                            .multipoint_paired_devices
                            .as_ref()
                            .and_then(|devs| devs.get(dev_idx).cloned());
                        let pinned = d.as_ref().map(|dev| s.capabilities.pinned_mac.as_deref() == Some(&dev.mac)).unwrap_or(false);
                        (d, pinned)
                    };
                    if let Some(d) = target_dev {
                        if d.connected {
                            log_trace(&format!("WM_MENURBUTTONUP: spawning context menu for {}, is_pinned={}", d.name, is_pinned));
                            let h_context = CreatePopupMenu();
                            let pin_label = format!("pin focus to {}", d.name);
                            let pin_check = if is_pinned { MF_CHECKED } else { MF_UNCHECKED };
                            AppendMenuW(h_context, MF_STRING | pin_check, ID_POPUP_PIN_FOCUS, to_wide(&pin_label).as_ptr());
                            AppendMenuW(h_context, MF_SEPARATOR, 0, null_mut());
                            let disc_label = format!("disconnect {}", d.name);
                            AppendMenuW(h_context, MF_STRING, ID_POPUP_DISCONNECT, to_wide(&disc_label).as_ptr());

                            let mut pt = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut pt);
                            SetForegroundWindow(hwnd);

                            let action = TrackPopupMenuEx(
                                h_context,
                                TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_RECURSE,
                                pt.x,
                                pt.y,
                                hwnd,
                                null_mut(),
                            ) as usize;

                            DestroyMenu(h_context);
                            log_trace(&format!("WM_MENURBUTTONUP: action={}", action));

                            if action == ID_POPUP_PIN_FOCUS {
                                if is_pinned {
                                    manager.unpin_focus(d.mac);
                                } else {
                                    manager.pin_focus(d.mac);
                                }
                                EndMenu();
                            } else if action == ID_POPUP_DISCONNECT {
                                manager.disconnect_device(d.mac);
                                EndMenu();
                            }
                            return 0;
                        }
                    }
                }
            }
            0
        }
        WM_DESTROY | WM_CLOSE => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn build_tooltip(state: &crate::state::DeviceState) -> String {
    if state.connected {
        let bat_str = match &state.battery_topology {
            Some(BatteryTopology::Tws { left, right, case }) => {
                if let Some(c) = case {
                    format!("L:{}% R:{}% C:{}%", left.level, right.level, c.level)
                } else {
                    format!("L:{}% R:{}%", left.level, right.level)
                }
            }
            _ => {
                if let Some(b) = state.battery {
                    format!("{}%", b.level)
                } else {
                    "connected".to_string()
                }
            }
        };
        let anc_str = match state.anc_mode {
            Some(AncMode::NoiseCanceling) => " [NC]",
            Some(AncMode::AmbientSound { .. }) => " [ASM]",
            Some(AncMode::Off) => " [OFF]",
            None => "",
        };
        let conn_extra = if let Some(ref c_mac) = state.capabilities.connecting_mac {
            if let Some(ref devs) = state.capabilities.multipoint_paired_devices {
                if let Some(target) = devs.iter().find(|d| &d.mac == c_mac) {
                    format!(" (connecting to {}...)", target.name)
                } else {
                    " (connecting...)".to_string()
                }
            } else {
                " (connecting...)".to_string()
            }
        } else {
            String::new()
        };
        format!("{}: {}{}{}", state.device_name, bat_str, anc_str, conn_extra)
    } else if state.connecting {
        format!("{}: connecting...", state.device_name)
    } else {
        format!("{}: disconnected", state.device_name)
    }
}

pub unsafe fn force_readd_tray_icon(hwnd: HWND) {
    if let Some(ref manager) = GLOBAL_MANAGER {
        let state = manager.state.lock().unwrap().clone();
        let is_tws = state.capabilities.has_dual_battery;

        let new_icon = render_tray_icon(
            state.connected,
            &state.battery_topology,
            state.battery,
            is_tws,
        ).unwrap_or_else(create_fallback_headphone_icon);

        let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = new_icon;

        let tooltip = build_tooltip(&state);
        let wide = to_wide(&tooltip);
        let len = wide.len().min(nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&wide[..len]);
        nid.szTip[len] = 0;

        if Shell_NotifyIconW(NIM_ADD, &mut nid) == 0 {
            Shell_NotifyIconW(NIM_MODIFY, &mut nid);
        }

        if !PREV_ICON.is_null() && PREV_ICON != new_icon {
            destroy_icon_safe(PREV_ICON);
        }
        PREV_ICON = new_icon;
    }
}

unsafe fn update_tray_icon(hwnd: HWND) {
    if let Some(ref manager) = GLOBAL_MANAGER {
        let state = manager.state.lock().unwrap().clone();
        let is_tws = state.capabilities.has_dual_battery;

        let new_icon = render_tray_icon(
            state.connected,
            &state.battery_topology,
            state.battery,
            is_tws,
        );

        let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;

        if let Some(h_icon) = new_icon {
            nid.uFlags |= NIF_ICON;
            nid.hIcon = h_icon;
        }

        let tooltip = build_tooltip(&state);
        let wide = to_wide(&tooltip);
        let len = wide.len().min(nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&wide[..len]);
        nid.szTip[len] = 0;

        if Shell_NotifyIconW(NIM_MODIFY, &mut nid) == 0 {
            log_trace("update_tray_icon: NIM_MODIFY returned 0, recovering with NIM_ADD");
            Shell_NotifyIconW(NIM_ADD, &mut nid);
        }

        if let Some(h_icon) = new_icon {
            if !PREV_ICON.is_null() && PREV_ICON != h_icon {
                destroy_icon_safe(PREV_ICON);
            }
            PREV_ICON = h_icon;
        }
    }
}

unsafe fn append_footer_menu_items(h_menu: HMENU) {
    let autostart_check = if is_run_on_startup() { MF_CHECKED } else { MF_UNCHECKED };
    AppendMenuW(h_menu, MF_STRING | autostart_check, ID_STARTUP_TOGGLE, to_wide("run on start-up").as_ptr());

    let h_other = CreatePopupMenu();
    AppendMenuW(h_other, MF_STRING, ID_ABOUT, to_wide("about").as_ptr());
    AppendMenuW(h_other, MF_STRING, ID_SOURCE_CODE, to_wide("source code").as_ptr());
    AppendMenuW(h_other, MF_SEPARATOR, 0, null_mut());
    let debug_flag = if is_debug_logging() { MF_CHECKED } else { MF_UNCHECKED };
    AppendMenuW(h_other, MF_STRING | debug_flag, ID_DEBUG_TOGGLE, to_wide("debug logging").as_ptr());
    AppendMenuW(h_other, MF_STRING, ID_OPEN_LOGS, to_wide("open logs folder").as_ptr());
    AppendMenuW(h_other, MF_SEPARATOR, 0, null_mut());
    AppendMenuW(h_other, MF_STRING, ID_RESTART, to_wide("restart").as_ptr());
    AppendMenuW(h_other, MF_STRING, ID_UNINSTALL, to_wide("uninstall").as_ptr());

    AppendMenuW(h_menu, MF_POPUP, h_other as usize, to_wide("other ›").as_ptr());
    AppendMenuW(h_menu, MF_SEPARATOR, 0, null_mut());
    AppendMenuW(h_menu, MF_STRING, ID_EXIT, to_wide("quit").as_ptr());
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let manager = match GLOBAL_MANAGER {
        Some(ref m) => m.clone(),
        None => return,
    };
    let state = manager.state.lock().unwrap().clone();

    LAST_CONNECTED = state.connected;

    apply_window_dark_mode(hwnd);
    flush_menu_themes();

    let h_menu = CreatePopupMenu();
    if h_menu.is_null() {
        return;
    }

    if state.connected {
        // 1. Header: Status & Device
        let status_str = "status: connected";
        AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_HEADER_STATUS, to_wide(status_str).as_ptr());
        if state.capabilities.paired_headsets.len() > 1 {
            let h_dev_menu = CreatePopupMenu();
            for (idx, dev) in state.capabilities.paired_headsets.iter().enumerate() {
                let is_current = dev.name.eq_ignore_ascii_case(&state.device_name);
                let check = if is_current { MF_CHECKED } else { MF_UNCHECKED };
                let tag = if dev.connected { " (connected)" } else { " (paired)" };
                let label = format!("{}{}", dev.name, tag);
                let item_id = ID_HEADSET_BASE + idx;
                AppendMenuW(h_dev_menu, MF_STRING | check, item_id, to_wide(&label).as_ptr());
            }
            let dev_label = format!("device: {}", state.device_name);
            AppendMenuW(h_menu, MF_POPUP, h_dev_menu as usize, to_wide(&dev_label).as_ptr());
        } else {
            let dev_str = format!("device: {}", state.device_name);
            AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_HEADER_DEV, to_wide(&dev_str).as_ptr());
        }

        // Separator
        AppendMenuW(h_menu, MF_SEPARATOR, 0, null_mut());

        // 2. Battery Section
        match &state.battery_topology {
            Some(BatteryTopology::Tws { left, right, case }) => {
                let left_str = format!("left: {}%{}", left.level, if left.charging { " ⚡" } else { "" });
                AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_BAT_LEFT, to_wide(&left_str).as_ptr());

                let right_str = format!("right: {}%{}", right.level, if right.charging { " ⚡" } else { "" });
                AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_BAT_RIGHT, to_wide(&right_str).as_ptr());

                if let Some(c) = case {
                    let case_str = format!("case: {}%{}", c.level, if c.charging { " ⚡" } else { "" });
                    AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_BAT_CASE, to_wide(&case_str).as_ptr());
                }
            }
            _ => {
                let bat_text = if let Some(bat) = state.battery {
                    format!("battery: {}%{}", bat.level, if bat.charging { " ⚡" } else { "" })
                } else {
                    "battery: --%".to_string()
                };
                AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_BAT_SINGLE, to_wide(&bat_text).as_ptr());
            }
        }

        // Separator
        AppendMenuW(h_menu, MF_SEPARATOR, 0, null_mut());

        // 3. Firmware Version (if present)
        if let Some(ref fw) = state.capabilities.firmware {
            let fw_str = format!("firmware: {}", fw);
            AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_FW_VER, to_wide(&fw_str).as_ptr());
        }

        // 4. ANC Mode Submenu
        let h_anc_menu = CreatePopupMenu();
        let cur_anc = state.anc_mode.unwrap_or(AncMode::NoiseCanceling);
        let (is_asm, cur_lvl, cur_voice) = match cur_anc {
            AncMode::AmbientSound { level, voice } => (true, level, voice),
            _ => (false, 20, false),
        };
        let nc_check = if cur_anc == AncMode::NoiseCanceling { MF_CHECKED } else { MF_UNCHECKED };
        let asm_check = if is_asm { MF_CHECKED } else { MF_UNCHECKED };
        let off_check = if cur_anc == AncMode::Off { MF_CHECKED } else { MF_UNCHECKED };

        AppendMenuW(h_anc_menu, MF_STRING | nc_check, ID_ANC_NC, to_wide("noise canceling").as_ptr());
        AppendMenuW(h_anc_menu, MF_STRING | asm_check, ID_ANC_ASM, to_wide("ambient sound").as_ptr());
        AppendMenuW(h_anc_menu, MF_STRING | off_check, ID_ANC_OFF, to_wide("off").as_ptr());

        let anc_label = match cur_anc {
            AncMode::NoiseCanceling => "anc: noise canceling".to_string(),
            AncMode::AmbientSound { level, voice } => {
                format!("anc: ambient sound (lvl {}{})", level, if voice { ", voice" } else { "" })
            }
            AncMode::Off => "anc: off".to_string(),
        };
        AppendMenuW(h_menu, MF_POPUP, h_anc_menu as usize, to_wide(&anc_label).as_ptr());

        // Ambient Sound Controls (Progressive disclosure: only rendered when in Ambient Sound mode)
        if is_asm {
            let h_asm_lvl_menu = CreatePopupMenu();
            let asm_levels = [
                (20, "level 20 (max)", ID_ASM_LEVEL_20),
                (15, "level 15", ID_ASM_LEVEL_15),
                (10, "level 10", ID_ASM_LEVEL_10),
                (5, "level 5", ID_ASM_LEVEL_5),
                (1, "level 1 (min)", ID_ASM_LEVEL_1),
            ];
            for (lvl, name, id) in asm_levels {
                let chk = if cur_lvl == lvl { MF_CHECKED } else { MF_UNCHECKED };
                AppendMenuW(h_asm_lvl_menu, MF_STRING | chk, id, to_wide(name).as_ptr());
            }
            let asm_lvl_label = format!("ambient sound level: {}", cur_lvl);
            AppendMenuW(h_menu, MF_POPUP, h_asm_lvl_menu as usize, to_wide(&asm_lvl_label).as_ptr());

            let voice_check = if cur_voice { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | voice_check, ID_ASM_VOICE_FOCUS, to_wide("focus on voice").as_ptr());
        }

        // 5. Equalizer Submenu
        let h_eq_menu = CreatePopupMenu();
        let cur_preset = state.eq_info.as_ref().map(|e| e.preset).unwrap_or(EqPreset::Off);

        let eq_items = [
            (EqPreset::Off, ID_EQ_OFF),
            (EqPreset::BassBoost, ID_EQ_BASS_BOOST),
            (EqPreset::TrebleBoost, ID_EQ_TREBLE_BOOST),
            (EqPreset::Vocal, ID_EQ_VOCAL),
            (EqPreset::Speech, ID_EQ_SPEECH),
            (EqPreset::Relaxed, ID_EQ_RELAXED),
            (EqPreset::Bright, ID_EQ_BRIGHT),
            (EqPreset::Manual, ID_EQ_MANUAL),
        ];

        for (preset, id) in eq_items {
            let check = if cur_preset == preset { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(
                h_eq_menu,
                MF_STRING | check,
                id,
                to_wide(preset.display_name()).as_ptr(),
            );
        }

        let eq_label = format!("equalizer: {}", cur_preset.display_name());
        AppendMenuW(h_menu, MF_POPUP, h_eq_menu as usize, to_wide(&eq_label).as_ptr());

        // Clear Bass Submenu
        let h_cb_menu = CreatePopupMenu();
        let cur_cb = state.eq_info.as_ref().map(|e| e.clear_bass).unwrap_or(0);

        for val in ((-10)..=10).rev() {
            let id = ID_CLEAR_BASS_BASE + (val + 10) as usize;
            let check = if cur_cb == val { MF_CHECKED } else { MF_UNCHECKED };
            let text = if val > 0 {
                format!("+{val}")
            } else {
                format!("{val}")
            };
            AppendMenuW(h_cb_menu, MF_STRING | check, id, to_wide(&text).as_ptr());
        }

        let cb_label = if cur_cb > 0 {
            format!("clear bass: +{}", cur_cb)
        } else {
            format!("clear bass: {}", cur_cb)
        };
        AppendMenuW(h_menu, MF_POPUP, h_cb_menu as usize, to_wide(&cb_label).as_ptr());

        // 6. Audio Quality (Codec & Connection Mode)
        if let Some(ref codec) = state.capabilities.codec {
            let codec_str = format!("audio quality: {}", codec);
            AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_CODEC, to_wide(&codec_str).as_ptr());
        }

        if state.capabilities.has_connection_mode {
            let h_cm_menu = CreatePopupMenu();
            let cur_cm = state.capabilities.connection_mode.unwrap_or(ConnectionMode::PriorityOnSoundQuality);
            let sq_check = if cur_cm == ConnectionMode::PriorityOnSoundQuality { MF_CHECKED } else { MF_UNCHECKED };
            let sc_check = if cur_cm == ConnectionMode::PriorityOnStableConnection { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_cm_menu, MF_STRING | sq_check, ID_CONN_SOUND_QUALITY, to_wide("priority on sound quality").as_ptr());
            AppendMenuW(h_cm_menu, MF_STRING | sc_check, ID_CONN_STABLE, to_wide("priority on stable connection").as_ptr());
            let cm_label = format!("connection mode: {}", cur_cm.display_name());
            AppendMenuW(h_menu, MF_POPUP, h_cm_menu as usize, to_wide(&cm_label).as_ptr());
        }

        // 7. Capability Submenus & Toggles
        if state.capabilities.has_multipoint {
            let h_mp_menu = CreatePopupMenu();
            let mp_enabled = state.capabilities.multipoint_enabled;
            let mp_check = if mp_enabled { MF_CHECKED } else { MF_UNCHECKED };

            AppendMenuW(
                h_mp_menu,
                MF_STRING | mp_check,
                ID_MULTIPOINT_TOGGLE,
                to_wide("connect to 2 devices simultaneously").as_ptr(),
            );

            if mp_enabled {
                AppendMenuW(h_mp_menu, MF_SEPARATOR, 0, null_mut());

                let mut has_conn = false;
                let mut has_disc = false;

                if let Some(ref devs) = state.capabilities.multipoint_paired_devices {
                    // Connected devices (interactive - click to switch audio)
                    let mut connected_devs: Vec<(usize, &crate::protocol::PairedDevice)> = devs
                        .iter()
                        .enumerate()
                        .filter(|(_, d)| d.connected)
                        .collect();
                    connected_devs.sort_by_key(|(_, d)| d.slot);

                    for &(orig_idx, d) in &connected_devs {
                        if !has_conn {
                            AppendMenuW(h_mp_menu, MF_STRING | MF_GRAYED | MF_DISABLED, 0, to_wide("connected:").as_ptr());
                            has_conn = true;
                        }
                        let is_pc = crate::bluetooth::is_this_pc(&d.name);
                        let pc_tag = if is_pc { " (this pc)" } else { "" };
                        let is_pinned = state.capabilities.pinned_mac.as_deref() == Some(&d.mac);
                        let pin_tag = if is_pinned { " 📌" } else { "" };
                        let play_tag = if d.is_playing { " 🔊" } else { "" };
                        let check_flag = if is_pinned || d.is_playing { MF_CHECKED } else { MF_UNCHECKED };
                        let item_label = format!("  {}{}{}{}", d.name, pc_tag, pin_tag, play_tag);
                        let item_id = ID_MULTIPOINT_DEV_BASE + orig_idx;
                        AppendMenuW(h_mp_menu, MF_STRING | check_flag, item_id, to_wide(&item_label).as_ptr());
                    }

                    // Paired (stored / not currently connected) devices
                    for (orig_idx, d) in devs.iter().enumerate().filter(|(_, d)| !d.connected) {
                        if !has_disc {
                            if has_conn {
                                AppendMenuW(h_mp_menu, MF_SEPARATOR, 0, null_mut());
                            }
                            AppendMenuW(h_mp_menu, MF_STRING | MF_GRAYED | MF_DISABLED, 0, to_wide("paired (tap to connect):").as_ptr());
                            has_disc = true;
                        }
                        let is_target_connecting = state.capabilities.connecting_mac.as_deref() == Some(&d.mac);
                        let item_label = if is_target_connecting {
                            format!("  {} (connecting...)", d.name)
                        } else {
                            format!("  {}", d.name)
                        };
                        let flags = if is_target_connecting {
                            MF_STRING | MF_GRAYED | MF_DISABLED
                        } else {
                            MF_STRING
                        };
                        let item_id = ID_MULTIPOINT_DEV_BASE + orig_idx;
                        AppendMenuW(h_mp_menu, flags, item_id, to_wide(&item_label).as_ptr());
                    }
                }

                if !has_conn {
                    // Fallback while device list query is in flight
                    AppendMenuW(h_mp_menu, MF_STRING | MF_GRAYED | MF_DISABLED, 0, to_wide("  loading device list...").as_ptr());
                }
            }

            let mp_label = if let Some(ref c_mac) = state.capabilities.connecting_mac {
                if let Some(ref devs) = state.capabilities.multipoint_paired_devices {
                    if let Some(target) = devs.iter().find(|d| &d.mac == c_mac) {
                        format!("connect to 2 devices: connecting to {}...", target.name.to_lowercase())
                    } else {
                        "connect to 2 devices: connecting...".to_string()
                    }
                } else {
                    "connect to 2 devices: connecting...".to_string()
                }
            } else if mp_enabled {
                "connect to 2 devices: on".to_string()
            } else {
                "connect to 2 devices: off".to_string()
            };
            AppendMenuW(
                h_menu,
                MF_POPUP,
                h_mp_menu as usize,
                to_wide(&mp_label).as_ptr(),
            );
        }

        if state.capabilities.has_dsee {
            let dsee_check = if state.capabilities.dsee_enabled { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | dsee_check, ID_DSEE_TOGGLE, to_wide("dsee").as_ptr());
        }

        if state.capabilities.has_voice_guidance {
            let vg_check = if state.capabilities.voice_guidance_enabled { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | vg_check, ID_VOICE_GUIDANCE_TOGGLE, to_wide("voice guidance").as_ptr());
        }

        if state.capabilities.has_touch_sensor {
            let ts_check = if state.capabilities.touch_sensor_enabled { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | ts_check, ID_TOUCH_SENSOR_TOGGLE, to_wide("touch sensor control").as_ptr());
        }

        if state.capabilities.has_in_ear_detection {
            let ws_check = if state.capabilities.in_ear_detection_enabled { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | ws_check, ID_WEARING_SENSOR_TOGGLE, to_wide("in-ear detection").as_ptr());
        }

        if state.capabilities.has_speak_to_chat {
            let stc_check = if state.capabilities.speak_to_chat_enabled { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(h_menu, MF_STRING | stc_check, ID_SPEAK_TO_CHAT_TOGGLE, to_wide("speak-to-chat").as_ptr());
        }

        if state.capabilities.has_auto_power_off {
            let h_apo_menu = CreatePopupMenu();
            let cur_apo = state.capabilities.auto_power_off_index.min(5);

            let apo_items = [
                ID_APO_0, ID_APO_1, ID_APO_2, ID_APO_3, ID_APO_4, ID_APO_5,
            ];

            for (idx, &id) in apo_items.iter().enumerate() {
                let check = if cur_apo == idx { MF_CHECKED } else { MF_UNCHECKED };
                let label = APO_NAMES.get(idx).unwrap_or(&"unknown");
                AppendMenuW(h_apo_menu, MF_STRING | check, id, to_wide(label).as_ptr());
            }

            let apo_cur_name = APO_NAMES.get(cur_apo).unwrap_or(&"unknown");
            let apo_label = format!("auto power off: {}", apo_cur_name);
            AppendMenuW(h_menu, MF_POPUP, h_apo_menu as usize, to_wide(&apo_label).as_ptr());
        }

        // Separator
        AppendMenuW(h_menu, MF_SEPARATOR, 0, null_mut());

        // 8. Refresh & Footer
        AppendMenuW(h_menu, MF_STRING, ID_REFRESH, to_wide("refresh now").as_ptr());
        append_footer_menu_items(h_menu);
    } else {
        // Disconnected or connecting state
        let status_str = if state.connecting {
            "status: connecting..."
        } else {
            "status: disconnected"
        };
        AppendMenuW(h_menu, MF_STRING | MF_GRAYED | MF_DISABLED, ID_HEADER_STATUS, to_wide(status_str).as_ptr());
        if state.capabilities.paired_headsets.len() > 1 {
            let h_dev_menu = CreatePopupMenu();
            for (idx, dev) in state.capabilities.paired_headsets.iter().enumerate() {
                let is_current = dev.name.eq_ignore_ascii_case(&state.device_name);
                let check = if is_current { MF_CHECKED } else { MF_UNCHECKED };
                let tag = if dev.connected { " (connected)" } else { " (paired)" };
                let label = format!("{}{}", dev.name, tag);
                let item_id = ID_HEADSET_BASE + idx;
                AppendMenuW(h_dev_menu, MF_STRING | check, item_id, to_wide(&label).as_ptr());
            }
            let dev_label = format!("device: {}", state.device_name);
            AppendMenuW(h_menu, MF_POPUP, h_dev_menu as usize, to_wide(&dev_label).as_ptr());
        }

        AppendMenuW(h_menu, MF_SEPARATOR, 0, null_mut());

        let refresh_flags = if state.connecting {
            MF_STRING | MF_GRAYED | MF_DISABLED
        } else {
            MF_STRING
        };
        AppendMenuW(h_menu, refresh_flags, ID_REFRESH, to_wide("refresh now").as_ptr());
        append_footer_menu_items(h_menu);
    }

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);

    let choice = TrackPopupMenuEx(
        h_menu,
        TPM_RETURNCMD,
        pt.x,
        pt.y,
        hwnd,
        null_mut(),
    ) as usize;

    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(h_menu);

    if choice != 0 {
        handle_menu_command(hwnd, choice);
    }
}

#[repr(C)]
struct DRAWITEMSTRUCT {
    ctl_type: u32,
    ctl_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    _pad: u32,
    hwnd_item: HWND,
    hdc: HDC,
    rc_item: RECT,
    item_data: usize,
}

const ODS_SELECTED: u32 = 0x0001;
static APP_ICON_BGRA: &[u8] = include_bytes!("../assets/app_32.bin");

struct AboutWindowData {
    reg_font: HFONT,
    bold_font: HFONT,
    dark_brush: HBRUSH,
    app_bmp: HBITMAP,
    is_dark: bool,
    copy_text: String,
    h_copy_btn: HWND,
}

static mut ABOUT_HWND: HWND = null_mut();
static mut ABOUT_DATA: Option<AboutWindowData> = None;

pub unsafe extern "system" fn about_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let hdc = wparam as HDC;
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            if let Some(ref data) = ABOUT_DATA {
                if data.is_dark {
                    FillRect(hdc, &rect, data.dark_brush);
                } else {
                    FillRect(hdc, &rect, GetSysColorBrush(COLOR_BTNFACE));
                }
            } else {
                FillRect(hdc, &rect, GetSysColorBrush(COLOR_BTNFACE));
            }
            1
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(ref data) = ABOUT_DATA {
                if !data.app_bmp.is_null() {
                    let mem_dc = CreateCompatibleDC(hdc);
                    let old_bmp = SelectObject(mem_dc, data.app_bmp as _);
                    let bf = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    AlphaBlend(hdc, 24, 20, 32, 32, mem_dc, 0, 0, 32, 32, bf);
                    SelectObject(mem_dc, old_bmp);
                    DeleteDC(mem_dc);
                }
                let pen = if data.is_dark {
                    CreatePen(PS_SOLID, 1, rgb(52, 52, 52))
                } else {
                    CreatePen(PS_SOLID, 1, rgb(218, 218, 218))
                };
                let old_pen = SelectObject(hdc, pen as _);
                MoveToEx(hdc, 24, 114, null_mut());
                LineTo(hdc, 456, 114);
                SelectObject(hdc, old_pen);
                DeleteObject(pen as _);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_DRAWITEM => {
            let dis = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
            if let Some(ref data) = ABOUT_DATA {
                let is_selected = (dis.item_state & ODS_SELECTED) != 0;

                // 1. Fill entire rectangle with window background so corners outside RoundRect are seamless
                if data.is_dark {
                    FillRect(dis.hdc, &dis.rc_item, data.dark_brush);
                } else {
                    FillRect(dis.hdc, &dis.rc_item, GetSysColorBrush(COLOR_BTNFACE));
                }

                let (bg, border, text_col) = if data.is_dark {
                    if is_selected {
                        (rgb(34, 34, 34), rgb(54, 54, 54), rgb(200, 200, 200))
                    } else {
                        (rgb(46, 46, 46), rgb(62, 62, 62), rgb(255, 255, 255))
                    }
                } else {
                    if is_selected {
                        (rgb(220, 220, 220), rgb(160, 160, 160), rgb(0, 0, 0))
                    } else {
                        (rgb(250, 250, 250), rgb(205, 205, 205), rgb(0, 0, 0))
                    }
                };

                let brush = CreateSolidBrush(bg);
                let pen = CreatePen(PS_SOLID, 1, border);
                let old_brush = SelectObject(dis.hdc, brush as _);
                let old_pen = SelectObject(dis.hdc, pen as _);

                RoundRect(
                    dis.hdc,
                    dis.rc_item.left,
                    dis.rc_item.top,
                    dis.rc_item.right,
                    dis.rc_item.bottom,
                    8,
                    8,
                );

                SelectObject(dis.hdc, old_brush);
                SelectObject(dis.hdc, old_pen);
                DeleteObject(brush as _);
                DeleteObject(pen as _);

                let mut buf = [0u16; 64];
                let len = GetWindowTextW(dis.hwnd_item, buf.as_mut_ptr(), 64);
                if len > 0 {
                    SetBkMode(dis.hdc, 1 /* TRANSPARENT */);
                    SetBkColor(dis.hdc, bg);
                    SetTextColor(dis.hdc, text_col);
                    let old_font = SelectObject(dis.hdc, data.reg_font as _);
                    let mut rc = dis.rc_item;
                    DrawTextW(
                        dis.hdc,
                        buf.as_ptr(),
                        len,
                        &mut rc,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    SelectObject(dis.hdc, old_font);
                }
            }
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            let hdc = wparam as HDC;
            if let Some(ref data) = ABOUT_DATA {
                if data.is_dark {
                    SetBkMode(hdc, 1 /* TRANSPARENT */);
                    SetBkColor(hdc, rgb(32, 32, 32));
                    SetTextColor(hdc, rgb(235, 235, 235));
                    return data.dark_brush as LRESULT;
                } else {
                    SetBkColor(hdc, GetSysColor(COLOR_BTNFACE));
                    SetTextColor(hdc, GetSysColor(COLOR_WINDOWTEXT));
                    return GetSysColorBrush(COLOR_BTNFACE) as LRESULT;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_COMMAND => {
            let cmd_id = (wparam & 0xFFFF) as usize;
            match cmd_id {
                3001 => {
                    if let Some(ref data) = ABOUT_DATA {
                        copy_to_clipboard(hwnd, &data.copy_text);
                        if !data.h_copy_btn.is_null() {
                            SetWindowTextW(data.h_copy_btn, to_wide("Copied!").as_ptr());
                        }
                    }
                    0
                }
                3002 => {
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if let Some(data) = ABOUT_DATA.take() {
                if !data.reg_font.is_null() {
                    DeleteObject(data.reg_font as _);
                }
                if !data.bold_font.is_null() {
                    DeleteObject(data.bold_font as _);
                }
                if !data.dark_brush.is_null() {
                    DeleteObject(data.dark_brush as _);
                }
                if !data.app_bmp.is_null() {
                    DeleteObject(data.app_bmp as _);
                }
            }
            ABOUT_HWND = null_mut();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub unsafe fn show_about_dialog(owner: HWND, manager: &DeviceManager) {
    if !ABOUT_HWND.is_null() && IsWindow(ABOUT_HWND) != 0 {
        SetForegroundWindow(ABOUT_HWND);
        return;
    }

    let h_inst = GetModuleHandleW(null_mut());
    let class_name = to_wide("SonyConnectAboutWndClass");

    let is_dark = is_system_dark_mode();
    let dark_brush = CreateSolidBrush(rgb(32, 32, 32));
    let (reg_font, bold_font) = get_system_ui_fonts();

    let diag = manager.get_diagnostics();
    let hours = diag.uptime_secs / 3600;
    let mins = (diag.uptime_secs % 3600) / 60;
    let secs = diag.uptime_secs % 60;
    let uptime_str = if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    };

    let copy_text = format!(
        "{}\n\
         Lightweight Windows tray app for Sony headphones and earbuds. Battery, ANC, EQ, Clear Bass, and multipoint controls. Pure Rust, one tiny executable.\n\n\
         author: Dimu (@pdimu)\n\
         co-author: Google Gemini 3.8 Flash\n\
         source: https://github.com/pdimu/connect-tray\n\n\
         diagnostics:\n\
         • uptime: {}\n\
         • active device: {} ({})\n\
         • protocol: {}\n\
         • session packets: {} sent, {} received\n",
        APP_TITLE_VERSION,
        uptime_str,
        diag.device_name,
        diag.device_mac,
        diag.protocol_version,
        diag.packets_sent,
        diag.packets_recv
    );

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = 32;
    bmi.bmiHeader.biHeight = -32; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB as u32;

    let mut bits_ptr: *mut std::ffi::c_void = null_mut();
    let hdc_screen = CreateCompatibleDC(null_mut());
    let app_bmp = CreateDIBSection(
        hdc_screen,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits_ptr,
        null_mut(),
        0,
    );
    DeleteDC(hdc_screen);

    if !app_bmp.is_null() && !bits_ptr.is_null() {
        std::ptr::copy_nonoverlapping(APP_ICON_BGRA.as_ptr(), bits_ptr as *mut u8, 4096);
    }

    // Populate ABOUT_DATA before creating any windows so all paint messages get it
    ABOUT_DATA = Some(AboutWindowData {
        reg_font,
        bold_font,
        dark_brush,
        app_bmp,
        is_dark,
        copy_text,
        h_copy_btn: null_mut(),
    });

    let mut wc: WNDCLASSW = std::mem::zeroed();
    wc.lpfnWndProc = Some(about_window_proc);
    wc.hInstance = h_inst;
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = if is_dark { dark_brush } else { GetSysColorBrush(COLOR_BTNFACE) };
    wc.lpszClassName = class_name.as_ptr();
    RegisterClassW(&wc);

    let width = 480;
    let height = 420;
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let x = (screen_w - width) / 2;
    let y = (screen_h - height) / 2;

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_DLGMODALFRAME,
        class_name.as_ptr(),
        to_wide(APP_TITLE_VERSION).as_ptr(),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        x,
        y,
        width,
        height,
        owner,
        null_mut(),
        h_inst,
        null_mut(),
    );

    if hwnd.is_null() {
        return;
    }

    ABOUT_HWND = hwnd;

    // Completely remove title bar icon
    SendMessageW(hwnd, WM_SETICON, 0, 0);
    SendMessageW(hwnd, WM_SETICON, 1, 0);

    set_dwm_dark_mode(hwnd, is_dark);
    apply_window_dark_mode(hwnd);

    // Header title
    let h_title = CreateWindowExW(
        0,
        to_wide("STATIC").as_ptr(),
        to_wide(APP_TITLE_VERSION).as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_LEFT,
        72,
        18,
        384,
        22,
        hwnd,
        null_mut(),
        h_inst,
        null_mut(),
    );
    SendMessageW(h_title, WM_SETFONT, bold_font as WPARAM, 1);

    // Header description
    let h_desc = CreateWindowExW(
        0,
        to_wide("STATIC").as_ptr(),
        to_wide("Lightweight Windows tray app for Sony headphones and earbuds. Battery, ANC, EQ, Clear Bass, and multipoint controls. Pure Rust, one tiny executable.").as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_LEFT,
        72,
        44,
        384,
        62,
        hwnd,
        null_mut(),
        h_inst,
        null_mut(),
    );
    SendMessageW(h_desc, WM_SETFONT, reg_font as WPARAM, 1);

    let create_row = |lbl: &str, val: &str, y_pos: i32| {
        let h_lbl = CreateWindowExW(
            0,
            to_wide("STATIC").as_ptr(),
            to_wide(lbl).as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            24,
            y_pos,
            140,
            20,
            hwnd,
            null_mut(),
            h_inst,
            null_mut(),
        );
        SendMessageW(h_lbl, WM_SETFONT, reg_font as WPARAM, 1);

        let h_edit = CreateWindowExW(
            0,
            to_wide("EDIT").as_ptr(),
            to_wide(val).as_ptr(),
            WS_CHILD | WS_VISIBLE | ES_READONLY | ES_AUTOHSCROLL | WS_TABSTOP,
            168,
            y_pos,
            288,
            20,
            hwnd,
            null_mut(),
            h_inst,
            null_mut(),
        );
        SendMessageW(h_edit, WM_SETFONT, reg_font as WPARAM, 1);
    };

    create_row("author:", "Dimu (@pdimu)", 124);
    create_row("co-author:", "Google Gemini 3.8 Flash", 148);
    create_row("source:", "https://github.com/pdimu/connect-tray", 172);

    let h_diag_hdr = CreateWindowExW(
        0,
        to_wide("STATIC").as_ptr(),
        to_wide("diagnostics:").as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_LEFT,
        24,
        202,
        432,
        20,
        hwnd,
        null_mut(),
        h_inst,
        null_mut(),
    );
    SendMessageW(h_diag_hdr, WM_SETFONT, bold_font as WPARAM, 1);

    create_row("• uptime:", &uptime_str, 226);
    create_row("• active device:", &format!("{} ({})", diag.device_name, diag.device_mac), 250);
    create_row("• protocol:", &format!("{}", diag.protocol_version), 274);
    create_row("• session packets:", &format!("{} sent, {} received", diag.packets_sent, diag.packets_recv), 298);

    let h_copy = CreateWindowExW(
        0,
        to_wide("BUTTON").as_ptr(),
        to_wide("Copy All").as_ptr(),
        (BS_OWNERDRAW as u32) | WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        252,
        336,
        104,
        30,
        hwnd,
        3001 as _,
        h_inst,
        null_mut(),
    );
    SendMessageW(h_copy, WM_SETFONT, reg_font as WPARAM, 1);

    let h_close = CreateWindowExW(
        0,
        to_wide("BUTTON").as_ptr(),
        to_wide("Close").as_ptr(),
        (BS_OWNERDRAW as u32) | WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        366,
        336,
        90,
        30,
        hwnd,
        3002 as _,
        h_inst,
        null_mut(),
    );
    SendMessageW(h_close, WM_SETFONT, reg_font as WPARAM, 1);

    if let Some(ref mut data) = ABOUT_DATA {
        data.h_copy_btn = h_copy;
    }

    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    SetForegroundWindow(hwnd);
}

struct ConfirmDialogData {
    reg_font: HFONT,
    dark_brush: HBRUSH,
    icon: HICON,
    is_dark: bool,
    result: Option<bool>,
}

static mut CONFIRM_DATA: Option<ConfirmDialogData> = None;
static mut CONFIRM_HWND: HWND = null_mut();

pub unsafe extern "system" fn confirm_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let hdc = wparam as HDC;
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            if let Some(ref data) = CONFIRM_DATA {
                if data.is_dark {
                    FillRect(hdc, &rect, data.dark_brush);
                } else {
                    FillRect(hdc, &rect, GetSysColorBrush(COLOR_BTNFACE));
                }
            } else {
                FillRect(hdc, &rect, GetSysColorBrush(COLOR_BTNFACE));
            }
            1
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(ref data) = CONFIRM_DATA {
                if !data.icon.is_null() {
                    DrawIconEx(hdc, 24, 24, data.icon, 32, 32, 0, null_mut(), 3 /* DI_NORMAL */);
                }
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_DRAWITEM => {
            let dis = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
            if let Some(ref data) = CONFIRM_DATA {
                let is_selected = (dis.item_state & ODS_SELECTED) != 0;

                // 1. Fill entire rectangle with window background so corners outside RoundRect are seamless
                if data.is_dark {
                    FillRect(dis.hdc, &dis.rc_item, data.dark_brush);
                } else {
                    FillRect(dis.hdc, &dis.rc_item, GetSysColorBrush(COLOR_BTNFACE));
                }

                let (bg, border, text_col) = if data.is_dark {
                    if is_selected {
                        (rgb(34, 34, 34), rgb(54, 54, 54), rgb(200, 200, 200))
                    } else {
                        (rgb(46, 46, 46), rgb(62, 62, 62), rgb(255, 255, 255))
                    }
                } else {
                    if is_selected {
                        (rgb(220, 220, 220), rgb(160, 160, 160), rgb(0, 0, 0))
                    } else {
                        (rgb(250, 250, 250), rgb(205, 205, 205), rgb(0, 0, 0))
                    }
                };

                let brush = CreateSolidBrush(bg);
                let pen = CreatePen(PS_SOLID, 1, border);
                let old_brush = SelectObject(dis.hdc, brush as _);
                let old_pen = SelectObject(dis.hdc, pen as _);

                RoundRect(
                    dis.hdc,
                    dis.rc_item.left,
                    dis.rc_item.top,
                    dis.rc_item.right,
                    dis.rc_item.bottom,
                    8,
                    8,
                );

                SelectObject(dis.hdc, old_brush);
                SelectObject(dis.hdc, old_pen);
                DeleteObject(brush as _);
                DeleteObject(pen as _);

                let mut buf = [0u16; 64];
                let len = GetWindowTextW(dis.hwnd_item, buf.as_mut_ptr(), 64);
                if len > 0 {
                    SetBkMode(dis.hdc, 1 /* TRANSPARENT */);
                    SetBkColor(dis.hdc, bg);
                    SetTextColor(dis.hdc, text_col);
                    let old_font = SelectObject(dis.hdc, data.reg_font as _);
                    let mut rc = dis.rc_item;
                    DrawTextW(
                        dis.hdc,
                        buf.as_ptr(),
                        len,
                        &mut rc,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                    SelectObject(dis.hdc, old_font);
                }
            }
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let hdc = wparam as HDC;
            if let Some(ref data) = CONFIRM_DATA {
                if data.is_dark {
                    SetBkMode(hdc, 1 /* TRANSPARENT */);
                    SetBkColor(hdc, rgb(32, 32, 32));
                    SetTextColor(hdc, rgb(235, 235, 235));
                    return data.dark_brush as LRESULT;
                } else {
                    SetBkColor(hdc, GetSysColor(COLOR_BTNFACE));
                    SetTextColor(hdc, GetSysColor(COLOR_WINDOWTEXT));
                    return GetSysColorBrush(COLOR_BTNFACE) as LRESULT;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_COMMAND => {
            let cmd_id = (wparam & 0xFFFF) as usize;
            match cmd_id {
                6 => {
                    // IDYES
                    if let Some(ref mut data) = CONFIRM_DATA {
                        data.result = Some(true);
                    }
                    DestroyWindow(hwnd);
                    0
                }
                7 => {
                    // IDNO
                    if let Some(ref mut data) = CONFIRM_DATA {
                        data.result = Some(false);
                    }
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        WM_CLOSE => {
            if let Some(ref mut data) = CONFIRM_DATA {
                data.result = Some(false);
            }
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            CONFIRM_HWND = null_mut();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub unsafe fn show_confirm_dialog(owner: HWND, title: &str, prompt: &str, is_warning: bool) -> bool {
    let is_dark = !crate::icon::is_system_light_theme();
    let h_inst = GetModuleHandleW(null_mut());

    let icon_id = if is_warning {
        32515 as *const u16 // IDI_WARNING / IDI_EXCLAMATION
    } else {
        32514 as *const u16 // IDI_QUESTION
    };
    let icon = load_modern_system_icon(icon_id, 32, 32);

    let (reg_font, _) = get_system_ui_fonts();
    let dark_brush = CreateSolidBrush(rgb(32, 32, 32));

    CONFIRM_DATA = Some(ConfirmDialogData {
        reg_font,
        dark_brush,
        icon,
        is_dark,
        result: None,
    });

    let class_name = to_wide("ConnectTrayConfirm");
    let mut wc: WNDCLASSW = std::mem::zeroed();
    wc.lpfnWndProc = Some(confirm_window_proc);
    wc.hInstance = h_inst;
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = if is_dark { dark_brush } else { GetSysColorBrush(COLOR_BTNFACE) };
    wc.lpszClassName = class_name.as_ptr();
    RegisterClassW(&wc);

    let width = 450;
    let height = 195;
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let x = (screen_w - width) / 2;
    let y = (screen_h - height) / 2;

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_DLGMODALFRAME,
        class_name.as_ptr(),
        to_wide(title).as_ptr(),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        x,
        y,
        width,
        height,
        owner,
        null_mut(),
        h_inst,
        null_mut(),
    );

    if hwnd.is_null() {
        if let Some(data) = CONFIRM_DATA.take() {
            if !data.reg_font.is_null() {
                DeleteObject(data.reg_font as _);
            }
            if !data.dark_brush.is_null() {
                DeleteObject(data.dark_brush as _);
            }
            destroy_icon_safe(data.icon);
        }
        return false;
    }

    CONFIRM_HWND = hwnd;

    // Clear title bar icon
    SendMessageW(hwnd, WM_SETICON, 0, 0);
    SendMessageW(hwnd, WM_SETICON, 1, 0);

    if is_dark {
        apply_window_dark_mode(hwnd);
        set_dwm_dark_mode(hwnd, true);
    }

    // Static text control
    let h_static = CreateWindowExW(
        0,
        to_wide("STATIC").as_ptr(),
        to_wide(prompt).as_ptr(),
        WS_CHILD | WS_VISIBLE | SS_LEFT,
        74,
        22,
        346,
        88,
        hwnd,
        null_mut(),
        h_inst,
        null_mut(),
    );
    SendMessageW(h_static, WM_SETFONT, reg_font as WPARAM, 1);

    // [ Yes ] and [ No ] buttons
    let btn_w = 84;
    let btn_h = 30;
    let btn_y = 118;
    let no_x = 342;
    let yes_x = 248;

    let h_yes = CreateWindowExW(
        0,
        to_wide("BUTTON").as_ptr(),
        to_wide("Yes").as_ptr(),
        (BS_OWNERDRAW as u32) | WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        yes_x,
        btn_y,
        btn_w,
        btn_h,
        hwnd,
        6 as _, // IDYES
        h_inst,
        null_mut(),
    );
    SendMessageW(h_yes, WM_SETFONT, reg_font as WPARAM, 1);

    let h_no = CreateWindowExW(
        0,
        to_wide("BUTTON").as_ptr(),
        to_wide("No").as_ptr(),
        (BS_OWNERDRAW as u32) | WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        no_x,
        btn_y,
        btn_w,
        btn_h,
        hwnd,
        7 as _, // IDNO
        h_inst,
        null_mut(),
    );
    SendMessageW(h_no, WM_SETFONT, reg_font as WPARAM, 1);

    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    SetForegroundWindow(hwnd);

    let owner_valid = !owner.is_null() && IsWindow(owner) != 0;
    if owner_valid {
        EnableWindow(owner, 0);
    }

    let mut msg: MSG = std::mem::zeroed();
    while !CONFIRM_HWND.is_null() && GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        if msg.message == WM_KEYDOWN {
            if msg.wParam as u32 == VK_ESCAPE {
                if let Some(ref mut d) = CONFIRM_DATA {
                    d.result = Some(false);
                }
                DestroyWindow(hwnd);
                break;
            } else if msg.wParam as u32 == 0x0D /* VK_RETURN */ {
                if let Some(ref mut d) = CONFIRM_DATA {
                    d.result = Some(true);
                }
                DestroyWindow(hwnd);
                break;
            }
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    if owner_valid {
        EnableWindow(owner, 1);
        SetForegroundWindow(owner);
    }

    let final_res = CONFIRM_DATA.as_ref().and_then(|d| d.result).unwrap_or(false);

    if let Some(data) = CONFIRM_DATA.take() {
        if !data.reg_font.is_null() {
            DeleteObject(data.reg_font as _);
        }
        if !data.dark_brush.is_null() {
            DeleteObject(data.dark_brush as _);
        }
        destroy_icon_safe(data.icon);
    }

    final_res
}

unsafe fn handle_menu_command(hwnd: HWND, cmd_id: usize) {
    let manager = match GLOBAL_MANAGER {
        Some(ref m) => m,
        None => return,
    };

    match cmd_id {
        ID_ANC_NC => manager.set_anc(AncMode::NoiseCanceling),
        ID_ANC_ASM => {
            let (lvl, voice) = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { level, voice }) => (level, voice),
                    _ => (20, false),
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: lvl, voice });
        }
        ID_ANC_OFF => manager.set_anc(AncMode::Off),

        ID_ASM_VOICE_FOCUS => {
            let (lvl, voice) = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { level, voice }) => (level, !voice),
                    _ => (20, true),
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: lvl, voice });
        }
        ID_ASM_LEVEL_20 => {
            let voice = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { voice, .. }) => voice,
                    _ => false,
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: 20, voice });
        }
        ID_ASM_LEVEL_15 => {
            let voice = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { voice, .. }) => voice,
                    _ => false,
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: 15, voice });
        }
        ID_ASM_LEVEL_10 => {
            let voice = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { voice, .. }) => voice,
                    _ => false,
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: 10, voice });
        }
        ID_ASM_LEVEL_5 => {
            let voice = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { voice, .. }) => voice,
                    _ => false,
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: 5, voice });
        }
        ID_ASM_LEVEL_1 => {
            let voice = {
                let s = manager.state.lock().unwrap();
                match s.anc_mode {
                    Some(AncMode::AmbientSound { voice, .. }) => voice,
                    _ => false,
                }
            };
            manager.set_anc(AncMode::AmbientSound { level: 1, voice });
        }

        ID_CONN_SOUND_QUALITY => manager.set_connection_mode(ConnectionMode::PriorityOnSoundQuality),
        ID_CONN_STABLE => manager.set_connection_mode(ConnectionMode::PriorityOnStableConnection),

        ID_MULTIPOINT_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.multipoint_enabled
            };
            manager.set_multipoint(enabled);
        }
        ID_MULTIPOINT_SWITCH_THIS_PC => {
            manager.switch_playback_to_pc();
        }
        ID_MULTIPOINT_SWITCH_OTHER => {
            let target_mac = {
                let s = manager.state.lock().unwrap();
                s.capabilities.multipoint_paired_devices.as_ref().and_then(|devs| {
                    devs.iter()
                        .find(|d| d.connected && !crate::bluetooth::is_this_pc(&d.name))
                        .map(|d| d.mac.clone())
                })
            };
            if let Some(mac) = target_mac {
                manager.switch_playback_to_mac(mac);
            }
        }
        ID_VOICE_GUIDANCE_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.voice_guidance_enabled
            };
            manager.set_voice_guidance(enabled);
        }
        ID_TOUCH_SENSOR_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.touch_sensor_enabled
            };
            manager.set_touch_sensor(enabled);
        }

        ID_EQ_OFF => manager.set_eq(EqPreset::Off),
        ID_EQ_BASS_BOOST => manager.set_eq(EqPreset::BassBoost),
        ID_EQ_TREBLE_BOOST => manager.set_eq(EqPreset::TrebleBoost),
        ID_EQ_VOCAL => manager.set_eq(EqPreset::Vocal),
        ID_EQ_SPEECH => manager.set_eq(EqPreset::Speech),
        ID_EQ_RELAXED => manager.set_eq(EqPreset::Relaxed),
        ID_EQ_BRIGHT => manager.set_eq(EqPreset::Bright),
        ID_EQ_MANUAL => manager.set_eq(EqPreset::Manual),

        ID_DSEE_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.dsee_enabled
            };
            manager.set_dsee(enabled);
        }
        ID_WEARING_SENSOR_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.in_ear_detection_enabled
            };
            manager.set_wearing_sensor(enabled);
        }
        ID_SPEAK_TO_CHAT_TOGGLE => {
            let enabled = {
                let s = manager.state.lock().unwrap();
                !s.capabilities.speak_to_chat_enabled
            };
            manager.set_speak_to_chat(enabled);
        }

        ID_APO_0 => manager.set_apo(0),
        ID_APO_1 => manager.set_apo(1),
        ID_APO_2 => manager.set_apo(2),
        ID_APO_3 => manager.set_apo(3),
        ID_APO_4 => manager.set_apo(4),
        ID_APO_5 => manager.set_apo(5),

        cmd if (ID_CLEAR_BASS_BASE..=ID_CLEAR_BASS_BASE + 20).contains(&cmd) => {
            let val = (cmd - ID_CLEAR_BASS_BASE) as i8 - 10;
            manager.set_clear_bass(val);
        }

        cmd if (ID_MULTIPOINT_DEV_BASE..ID_MULTIPOINT_DEV_BASE + 32).contains(&cmd) => {
            let dev_idx = cmd - ID_MULTIPOINT_DEV_BASE;
            let target_dev = {
                let s = manager.state.lock().unwrap();
                s.capabilities
                    .multipoint_paired_devices
                    .as_ref()
                    .and_then(|devs| devs.get(dev_idx).cloned())
            };
            if let Some(d) = target_dev {
                if d.connected {
                    manager.switch_playback_to_mac(d.mac);
                } else {
                    manager.connect_device(d.mac);
                }
            }
        }

        cmd if (ID_HEADSET_BASE..ID_HEADSET_BASE + 20).contains(&cmd) => {
            let idx = cmd - ID_HEADSET_BASE;
            let target_headset = {
                let s = manager.state.lock().unwrap();
                s.capabilities.paired_headsets.get(idx).cloned()
            };
            if let Some(h) = target_headset {
                manager.switch_headset(h.address, h.name);
            }
        }

        ID_REFRESH => {
            let s = manager.state.lock().unwrap();
            if !s.connecting {
                drop(s);
                manager.refresh();
            }
        }
        ID_STARTUP_TOGGLE => {
            toggle_run_on_startup();
        }
        ID_ABOUT => {
            show_about_dialog(hwnd, manager);
        }
        ID_SOURCE_CODE => {
            let prompt = "Open project repository on GitHub?\n\nhttps://github.com/pdimu/connect-tray";
            if show_confirm_dialog(hwnd, "Sony Connect Tray", prompt, false) {
                open_url("https://github.com/pdimu/connect-tray");
            }
        }
        ID_DEBUG_TOGGLE => {
            toggle_debug_logging();
        }
        ID_OPEN_LOGS => {
            let dir = get_app_data_dir();
            open_url(&dir.to_string_lossy());
        }
        ID_RESTART => {
            if let Ok(exe) = std::env::current_exe() {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new(exe)
                    .creation_flags(0x08000000)
                    .spawn();
            }
            manager.shutdown();
            let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
            if !PREV_ICON.is_null() {
                destroy_icon_safe(PREV_ICON);
                PREV_ICON = null_mut();
            }
            DestroyWindow(hwnd);
            PostQuitMessage(0);
        }
        ID_UNINSTALL => {
            let prompt = "This will disable autostart, remove the Start Menu shortcut, and delete local application data.\n\nAre you sure you want to uninstall connect-tray?";
            if show_confirm_dialog(hwnd, "uninstall connect-tray", prompt, true) {
                let mut hkey: HKEY = null_mut();
                if RegOpenKeyExW(HKEY_CURRENT_USER, to_wide(RUN_KEY_PATH).as_ptr(), 0, KEY_WRITE, &mut hkey) == 0 {
                    let _ = RegDeleteValueW(hkey, to_wide(APP_REG_NAME).as_ptr());
                    RegCloseKey(hkey);
                }
                if let Some(lnk) = get_start_menu_shortcut_path() {
                    let _ = std::fs::remove_file(lnk);
                }
                let app_dir = get_app_data_dir();
                let _ = std::fs::remove_dir_all(&app_dir);

                manager.shutdown();
                let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
                nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = 1;
                Shell_NotifyIconW(NIM_DELETE, &mut nid);
                if !PREV_ICON.is_null() {
                    destroy_icon_safe(PREV_ICON);
                    PREV_ICON = null_mut();
                }
                DestroyWindow(hwnd);
                PostQuitMessage(0);
            }
        }
        ID_EXIT => {
            manager.shutdown();
            let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
            if !PREV_ICON.is_null() {
                destroy_icon_safe(PREV_ICON);
                PREV_ICON = null_mut();
            }
            DestroyWindow(hwnd);
            PostQuitMessage(0);
        }
        _ => {}
    }
}

pub fn attach_interactive_desktop() {
    unsafe {
        let user32 = windows_sys::Win32::System::LibraryLoader::LoadLibraryA(b"user32.dll\0".as_ptr());
        if user32.is_null() {
            return;
        }

        let fn_set_dpi: Option<unsafe extern "system" fn(isize) -> windows_sys::Win32::Foundation::BOOL> =
            std::mem::transmute(windows_sys::Win32::System::LibraryLoader::GetProcAddress(user32, b"SetProcessDpiAwarenessContext\0".as_ptr()));
        if let Some(set_dpi) = fn_set_dpi {
            set_dpi(-4); // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        }

        let fn_open_winsta: Option<unsafe extern "system" fn(*const u16, windows_sys::Win32::Foundation::BOOL, u32) -> windows_sys::Win32::Foundation::HANDLE> =
            std::mem::transmute(windows_sys::Win32::System::LibraryLoader::GetProcAddress(user32, b"OpenWindowStationW\0".as_ptr()));
        let fn_set_winsta: Option<unsafe extern "system" fn(windows_sys::Win32::Foundation::HANDLE) -> windows_sys::Win32::Foundation::BOOL> =
            std::mem::transmute(windows_sys::Win32::System::LibraryLoader::GetProcAddress(user32, b"SetProcessWindowStation\0".as_ptr()));
        let fn_open_desk: Option<unsafe extern "system" fn(*const u16, u32, windows_sys::Win32::Foundation::BOOL, u32) -> windows_sys::Win32::Foundation::HANDLE> =
            std::mem::transmute(windows_sys::Win32::System::LibraryLoader::GetProcAddress(user32, b"OpenDesktopW\0".as_ptr()));
        let fn_set_desk: Option<unsafe extern "system" fn(windows_sys::Win32::Foundation::HANDLE) -> windows_sys::Win32::Foundation::BOOL> =
            std::mem::transmute(windows_sys::Win32::System::LibraryLoader::GetProcAddress(user32, b"SetThreadDesktop\0".as_ptr()));

        if let (Some(open_winsta), Some(set_winsta), Some(open_desk), Some(set_desk)) =
            (fn_open_winsta, fn_set_winsta, fn_open_desk, fn_set_desk)
        {
            let winsta_name = to_wide("WinSta0");
            let hwinsta = open_winsta(winsta_name.as_ptr(), 0, 0x000F037F);
            if !hwinsta.is_null() {
                set_winsta(hwinsta);
            }

            let desk_name = to_wide("default");
            let hdesk = open_desk(desk_name.as_ptr(), 0, 0, 0x000F01FF);
            if !hdesk.is_null() {
                set_desk(hdesk);
            }
        }
    }
}

pub fn run_tray_app(manager: Arc<DeviceManager>) -> Result<(), String> {
    attach_interactive_desktop();

    unsafe {
        init_dark_mode_support();
        GLOBAL_MANAGER = Some(manager);

        let h_inst = GetModuleHandleW(null_mut());
        let class_name = to_wide("SonyConnectTrayWndClass");

        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_inst,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wc);
        WM_TASKBAR_CREATED = RegisterWindowMessageW(to_wide("TaskbarCreated").as_ptr());

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            to_wide("Sony Connect Tray").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            h_inst,
            null_mut(),
        );

        if hwnd.is_null() {
            return Err("failed to create tray utility window".to_string());
        }

        apply_window_dark_mode(hwnd);
        flush_menu_themes();

        GLOBAL_HWND = hwnd;

        let icon = render_tray_icon(false, &None, None, false)
            .unwrap_or_else(create_fallback_headphone_icon);
        PREV_ICON = icon;

        let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = icon;

        let initial_tip = to_wide("Sony Headset (connecting...)");
        let len = initial_tip.len().min(nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&initial_tip[..len]);
        nid.szTip[len] = 0;

        let mut added = false;
        for _ in 0..5 {
            if Shell_NotifyIconW(NIM_ADD, &mut nid) != 0 {
                added = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        if !added {
            log_trace("Shell_NotifyIconW failed to add");
            return Err("failed to register system tray icon".to_string());
        }
        log_trace("Shell_NotifyIconW added successfully. Entering message loop.");

        // Standard Win32 Message Loop
        let mut msg = std::mem::zeroed::<MSG>();
        loop {
            let res = GetMessageW(&mut msg, null_mut(), 0, 0);
            if res <= 0 {
                log_trace(&format!("GetMessageW exited with res: {res}"));
                break;
            }

            if msg.message == WM_KEYDOWN && (msg.wParam as u32) == VK_ESCAPE {
                if !ABOUT_HWND.is_null() && (msg.hwnd == ABOUT_HWND || IsChild(ABOUT_HWND, msg.hwnd) != 0) {
                    DestroyWindow(ABOUT_HWND);
                    continue;
                }
            }

            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Clean removal
        log_trace("Cleaning up tray icon and exiting run_tray_app");
        Shell_NotifyIconW(NIM_DELETE, &mut nid);
        if !PREV_ICON.is_null() {
            destroy_icon_safe(PREV_ICON);
            PREV_ICON = null_mut();
        }
        Ok(())
    }
}

pub fn notify_tray_update() {
    unsafe {
        if !GLOBAL_HWND.is_null() {
            PostMessageW(GLOBAL_HWND, WM_STATE_UPDATED, 0, 0);
        }
    }
}
