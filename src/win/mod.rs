//! Windows glue: injecting input, the active window, timer resolution,
//! console Ctrl+C. Everything that calls the Win32 API lives here.

pub mod console;
pub mod foreground;
pub mod send;

use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};

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
