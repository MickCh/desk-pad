//! Windows glue: injecting input, the window in front, the tray icon, the
//! console, autostart, timer resolution. Everything that calls the Win32
//! API lives here.

pub mod autostart;
pub mod console;
pub mod foreground;
pub mod icon;
pub mod send;
pub mod shell;
pub mod tray;

use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MessageBoxW,
};

/// `s` as a NUL-terminated UTF-16 string.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Copy `s` into a fixed-size UTF-16 field, cut to fit, NUL-terminated.
pub fn fill(dst: &mut [u16], s: &str) {
    let n = dst.len().saturating_sub(1);
    let mut len = 0;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(n)) {
        *d = c;
        len += 1;
    }
    if let Some(end) = dst.get_mut(len) {
        *end = 0;
    }
}

/// A message box, for when there is no console or tray icon to tell.
pub fn message_box(text: &str, warning: bool) {
    let text = wide(text);
    let title = wide("desk-pad");
    let icon = if warning {
        MB_ICONWARNING
    } else {
        MB_ICONINFORMATION
    };
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | icon) };
}

/// Whether another desk-pad runs in this Windows session. The first one
/// keeps a named mutex for its whole lifetime.
pub fn already_running() -> bool {
    let name = wide("Local\\desk-pad");
    // SAFETY: `name` is NUL-terminated; the handle is deliberately never
    // closed, so the mutex lives as long as the process.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

/// 1 ms timer resolution while alive, so the input loop can sleep ~4 ms
/// (the default resolution is 15.6 ms: too coarse for a smooth pointer).
pub struct TimerResolution;

impl TimerResolution {
    pub fn new() -> Self {
        // SAFETY: plain call without pointers; undone in `drop`.
        unsafe { timeBeginPeriod(1) };
        TimerResolution
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        // SAFETY: matches the `timeBeginPeriod(1)` in `new`.
        unsafe { timeEndPeriod(1) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_cuts_and_terminates() {
        let mut buf = [1u16; 4];
        fill(&mut buf, "abcdef");
        assert_eq!(buf, [97, 98, 99, 0]);
        fill(&mut buf, "x");
        assert_eq!(buf[..2], [120, 0]);
        let mut empty: [u16; 0] = [];
        fill(&mut empty, "x");
    }
}
