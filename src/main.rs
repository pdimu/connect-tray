#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use connect_tray::state::DeviceManager;
use connect_tray::tray;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG};

fn terminate_stale_connect_tray_instances() {
    let my_pid = std::process::id();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry = std::mem::zeroed::<PROCESSENTRY32W>();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let exe_name = String::from_utf16_lossy(&entry.szExeFile)
                        .trim_matches('\0')
                        .to_lowercase();
                    if (exe_name == "connect-tray.exe" || exe_name == "connect-tray")
                        && entry.th32ProcessID != my_pid
                    {
                        let h_proc = OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID);
                        if !h_proc.is_null() {
                            let _ = TerminateProcess(h_proc, 1);
                            let _ = CloseHandle(h_proc);
                        }
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }
    }
    // Give OS 100ms to clean up handles and release the RFCOMM Winsock socket
    std::thread::sleep(std::time::Duration::from_millis(100));
}

fn main() {
    let mutex_name = tray::to_wide("Local\\SonyConnectTrayInstanceMutex");
    let mut h_mutex = unsafe { CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // An instance is already running.
        // Attach to the interactive desktop session so FindWindowW can locate the utility window
        tray::attach_interactive_desktop();
        let class_name = tray::to_wide("SonyConnectTrayWndClass");
        let hwnd = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
        if !hwnd.is_null() {
            let mut res = 0usize;
            let ok = unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    tray::WM_RESTORE_TRAY,
                    0,
                    0,
                    SMTO_ABORTIFHUNG,
                    1000,
                    &mut res,
                )
            };
            if ok != 0 {
                // Successfully signaled existing running instance to restore icon & open menu!
                return;
            }
        }

        // If existing window is missing or hung/unresponsive, terminate stale instances and take over
        terminate_stale_connect_tray_instances();

        unsafe {
            if !h_mutex.is_null() {
                CloseHandle(h_mutex);
            }
            h_mutex = CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr());
        }
    }

    let _h_mutex = h_mutex;

    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        let msg = format!("panic: {info}\nbacktrace:\n{backtrace}\n");
        let log_path = tray::get_app_data_dir().join("panic.log");
        let _ = std::fs::write(log_path, msg);
    }));

    let manager = Arc::new(DeviceManager::start(tray::notify_tray_update));
    if let Err(e) = tray::run_tray_app(manager) {
        eprintln!("error: {e}");
    }
}
