//! The window in front: its program and title, to choose a profile.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// File name of the program, e.g. `WindowsTerminal.exe`; empty if
    /// Windows won't tell (e.g. an elevated process's window).
    pub exe: String,
    /// Window title; in Windows Terminal the title of the active tab.
    pub title: String,
}

/// The window in front, if any.
pub fn foreground() -> Option<Foreground> {
    // SAFETY: every buffer below is passed with its real length; the process
    // handle is closed before returning.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        let mut exe = String::new();
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                buf.as_mut_ptr(),
                &raw mut len,
            ) != 0
            {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                exe = path.rsplit('\\').next().unwrap_or_default().to_string();
            }
            CloseHandle(process);
        }
        Some(Foreground { exe, title })
    }
}
