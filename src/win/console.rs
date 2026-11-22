//! Ctrl+C and closing the console window: stop the input loop cleanly, so
//! no key desk-pad holds (Alt, Ctrl, a mouse button) stays pressed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::core::BOOL;

static STOP: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);

/// Install the handler. Afterwards Ctrl+C, Ctrl+Break, closing the console,
/// logging off and shutting down only set [`stop_requested`].
pub fn install() -> bool {
    // SAFETY: `handler` is a valid `extern "system"` function for the
    // lifetime of the process.
    unsafe { SetConsoleCtrlHandler(Some(handler), 1) != 0 }
}

/// Whether the loop should end.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// Tell the handler the loop has cleaned up, so the process may end.
pub fn done() {
    DONE.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn handler(_ctrl_type: u32) -> BOOL {
    STOP.store(true, Ordering::SeqCst);
    // On close/logoff/shutdown Windows ends the process when the handler
    // returns: give the loop a moment to release held keys first.
    let until = Instant::now() + Duration::from_secs(2);
    while !DONE.load(Ordering::SeqCst) && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    1
}
