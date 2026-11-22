//! Starting programs and opening files the way Explorer does
//! (`ShellExecute`): `run:` bindings and the tray menu.

use super::wide;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// `command` split into the program (or file, folder, URL) and its
/// arguments: the first word, or the first "quoted part".
pub fn split(command: &str) -> (&str, &str) {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return (&rest[..end], rest[end + 1..].trim());
    }
    match c.find(char::is_whitespace) {
        Some(i) => (&c[..i], c[i..].trim()),
        None => (c, ""),
    }
}

/// `%VARIABLES%` replaced by their values.
fn expand(s: &str) -> String {
    let src = wide(s);
    let mut buf = vec![0u16; 4096];
    // SAFETY: `src` is NUL-terminated; the size passed is the buffer's.
    let n = unsafe {
        ExpandEnvironmentStringsW(
            src.as_ptr(),
            buf.as_mut_ptr(),
            u32::try_from(buf.len()).unwrap_or(0),
        )
    } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n - 1])
}

fn shell_execute(file: &str, args: &str) -> Result<(), String> {
    let (wfile, wargs, verb) = (wide(file), wide(args), wide("open"));
    // SAFETY: the strings are NUL-terminated and outlive the call. COM is
    // initialised for this thread as ShellExecute asks (repeating it is
    // harmless).
    let r = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned(),
        );
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            wfile.as_ptr(),
            if args.is_empty() {
                null()
            } else {
                wargs.as_ptr()
            },
            null(),
            SW_SHOWNORMAL,
        )
    };
    // Values up to 32 are errors.
    if (r as isize) > 32 {
        Ok(())
    } else {
        Err(format!("cannot start \"{file}\" (error {})", r as isize))
    }
}

/// Start `command` (`run:`), on a thread of its own: starting a program
/// can take a moment, and the pad must not stall meanwhile.
pub fn run(command: &str) {
    let command = expand(command);
    std::thread::spawn(move || {
        let (file, args) = split(&command);
        if let Err(e) = shell_execute(file, args) {
            crate::logging::notice(&e);
        }
    });
}

/// Open a file with its program, or Notepad if it has none.
pub fn open(path: &Path) {
    let file = path.display().to_string();
    if shell_execute(&file, "").is_err() {
        let _ = shell_execute("notepad.exe", &format!("\"{file}\""));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_program_and_arguments() {
        assert_eq!(split("notepad.exe"), ("notepad.exe", ""));
        assert_eq!(split(" wt.exe -w 0 nt "), ("wt.exe", "-w 0 nt"));
        assert_eq!(
            split(r#""C:\Program Files\x\x.exe" --a "b c""#),
            (r"C:\Program Files\x\x.exe", r#"--a "b c""#)
        );
        assert_eq!(split("https://example.com"), ("https://example.com", ""));
    }

    #[test]
    fn variables_are_expanded() {
        let windir = std::env::var("windir").unwrap();
        assert_eq!(
            expand(r"%windir%\notepad.exe"),
            format!(r"{windir}\notepad.exe")
        );
        assert_eq!(expand("plain"), "plain");
    }
}
