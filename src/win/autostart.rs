//! "Start with Windows": a value in the current user's `Run` key.

use super::wide;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "desk-pad";

/// The command Windows should run: this exe, with `args`.
pub fn command(args: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe = format!("\"{}\"", exe.display());
    Some(if args.is_empty() {
        exe
    } else {
        format!("{exe} {args}")
    })
}

/// The command currently registered, if any.
fn registered() -> Option<String> {
    let (key, name) = (wide(RUN), wide(NAME));
    let mut buf = [0u16; 1024];
    let mut bytes = u32::try_from(size_of_val(&buf)).ok()?;
    // SAFETY: the strings are NUL-terminated; `bytes` is the buffer's size.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    if err != 0 {
        return None;
    }
    let len = (bytes as usize / 2).saturating_sub(1); // without the NUL
    Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
}

/// Whether desk-pad starts with Windows: as this exe, with `args`.
pub fn enabled(args: &str) -> bool {
    registered().is_some_and(|r| Some(r) == command(args))
}

/// Start with Windows (as this exe, with `args`), or not.
pub fn set(on: bool, args: &str) -> Result<(), String> {
    let (key, name) = (wide(RUN), wide(NAME));
    let err = if on {
        let cmd = command(args).ok_or("cannot find desk-pad's own file name")?;
        let data = wide(&cmd);
        let bytes = u32::try_from(data.len() * 2).map_err(|e| e.to_string())?;
        // SAFETY: the strings are NUL-terminated; `bytes` covers `data`.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        }
    } else {
        // SAFETY: the strings are NUL-terminated.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
            ERROR_FILE_NOT_FOUND => 0,
            e => e,
        }
    };
    if err == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot change \"start with Windows\" (registry error {err})"
        ))
    }
}
