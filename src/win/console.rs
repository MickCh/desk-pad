//! The console, for `--test` and `--console`: desk-pad is a Windows (GUI)
//! program, so it opens a console of its own when it needs one. Ctrl+C and
//! closing that console stop the input loop cleanly, so no key desk-pad
//! holds stays pressed.

use super::wide;
use crate::stop;
use std::ptr::{null, null_mut};
use std::time::Duration;
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetStdHandle, STD_ERROR_HANDLE,
    STD_OUTPUT_HANDLE, SetConsoleCtrlHandler, SetStdHandle,
};
use windows_sys::core::BOOL;

/// Whether standard output goes somewhere (a console, or a pipe or file
/// the parent set up, e.g. `desk-pad --test | more`).
pub fn has_stdout() -> bool {
    // SAFETY: plain call without pointers.
    let h = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    !h.is_null() && h != INVALID_HANDLE_VALUE
}

/// Use the console of the program that started desk-pad (for `--help` and
/// command-line errors).
pub fn attach_parent() -> bool {
    // SAFETY: plain call without pointers.
    let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 };
    attached && redirect()
}

/// Open a console window of desk-pad's own.
pub fn alloc() -> bool {
    // SAFETY: plain call without pointers.
    let allocated = unsafe { AllocConsole() != 0 };
    allocated && redirect()
}

/// Point standard output and error at the console.
fn redirect() -> bool {
    let name = wide("CONOUT$");
    // SAFETY: `name` is a NUL-terminated wide string; the handle stays open
    // for the rest of the process, as standard output.
    unsafe {
        let h = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, h) != 0 && SetStdHandle(STD_ERROR_HANDLE, h) != 0
    }
}

/// Afterwards Ctrl+C, Ctrl+Break and closing the console request a stop.
pub fn install_ctrl_handler() -> bool {
    // SAFETY: `handler` is a valid `extern "system"` function for the
    // lifetime of the process.
    unsafe { SetConsoleCtrlHandler(Some(handler), 1) != 0 }
}

unsafe extern "system" fn handler(_ctrl_type: u32) -> BOOL {
    stop::request();
    // On close Windows ends the process when the handler returns: give the
    // loop a moment to release held keys first.
    stop::wait_done(Duration::from_secs(2));
    1
}
