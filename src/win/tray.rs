//! The tray icon: shows the state (colour and tooltip), shows notices as
//! Windows notifications, and has the menu (pause, reload, open the
//! configuration and the log, start with Windows, quit).
//!
//! It runs on the main thread with a hidden window and its message loop;
//! the input loop runs on its own thread and talks to it through
//! [`Shared`] (state and notices, posted as window messages) and a channel
//! of [`Command`]s back.

use super::icon::{self, Icon};
use super::{autostart, fill, wide};
use crate::app::{Look, Status};
use crate::{logging, stop};
use std::cell::Cell;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW, MF_CHECKED,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SW_SHOWNORMAL, SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_DESTROY,
    WM_ENDSESSION, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

/// Requests from the tray menu to the input loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TogglePause,
    Reload,
}

/// What the input loop needs to talk to the tray icon.
pub struct Link {
    pub shared: Arc<Shared>,
    pub commands: Receiver<Command>,
}

/// State shared between the input loop and the tray icon.
#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    notices: Mutex<Vec<String>>,
    /// The tray window (0 until it exists).
    hwnd: AtomicIsize,
}

const WM_TRAY: u32 = WM_APP + 1;
const WM_STATUS: u32 = WM_APP + 2;
const WM_NOTICE: u32 = WM_APP + 3;
const WM_QUIT_TRAY: u32 = WM_APP + 4;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub fn set_status(&self, status: Status) {
        *lock(&self.status) = status;
        self.post(WM_STATUS);
    }

    /// Show `msg` as a notification.
    pub fn notice(&self, msg: &str) {
        lock(&self.notices).push(msg.to_string());
        self.post(WM_NOTICE);
    }

    /// Remove the icon and end the tray's message loop.
    pub fn quit(&self) {
        self.post(WM_QUIT_TRAY);
    }

    fn post(&self, msg: u32) {
        let hwnd = self.hwnd.load(Ordering::SeqCst) as HWND;
        if !hwnd.is_null() {
            // SAFETY: posting to a window handle is safe even if the window
            // is gone (the call then fails).
            unsafe { PostMessageW(hwnd, msg, 0, 0) };
        }
    }
}

/// Files the menu opens, and the arguments for "start with Windows".
pub struct Paths {
    pub config: PathBuf,
    pub log: Option<PathBuf>,
    pub autostart_args: String,
}

const ICON_ID: u32 = 1;
const ID_PAUSE: usize = 1;
const ID_RELOAD: usize = 2;
const ID_CONFIG: usize = 3;
const ID_LOG: usize = 4;
const ID_AUTOSTART: usize = 5;
const ID_QUIT: usize = 6;

/// Icon colours (0xRRGGBB) by state.
fn colour(look: Look) -> u32 {
    match look {
        Look::Active => 0x2E9E4F,      // green
        Look::Paused => 0x8A8A8A,      // grey
        Look::NoPad => 0xC0392B,       // red
        Look::Digital => 0xE0A800,     // amber
        Look::Passthrough => 0x3A7BD5, // blue
    }
}

const LOOKS: [Look; 5] = [
    Look::Active,
    Look::Paused,
    Look::NoPad,
    Look::Digital,
    Look::Passthrough,
];

struct Tray {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    paths: Paths,
    icons: Vec<(Look, Icon)>,
    /// Sent by Explorer when it (re)starts: the icon must be added again.
    taskbar_created: u32,
    /// Whether the icon is in the notification area.
    added: Cell<bool>,
}

impl Tray {
    fn data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let status = lock(&self.shared.status).clone();
        let look = status.look();
        let mut nid = NOTIFYICONDATAW {
            cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
            hWnd: hwnd,
            uID: ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self
                .icons
                .iter()
                .find(|(l, _)| *l == look)
                .map_or(null_mut(), |(_, i)| i.0),
            ..Default::default()
        };
        fill(&mut nid.szTip, &format!("desk-pad: {}", status.describe()));
        nid
    }

    fn add(&self, hwnd: HWND) {
        let nid = self.data(hwnd);
        // SAFETY: `nid` is fully initialised and outlives the call.
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const nid) } != 0;
        self.added.set(ok);
    }

    fn update(&self, hwnd: HWND) {
        if !self.added.get() {
            self.add(hwnd); // e.g. Explorer was not ready at start
            return;
        }
        let nid = self.data(hwnd);
        // SAFETY: as in `add`.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const nid) };
    }

    fn remove(&self, hwnd: HWND) {
        let nid = self.data(hwnd);
        // SAFETY: as in `add`.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const nid) };
        self.added.set(false);
    }

    /// Show the newest notice (earlier ones are in the log).
    fn notify(&self, hwnd: HWND) {
        let Some(msg) = lock(&self.shared.notices).drain(..).next_back() else {
            return;
        };
        let mut nid = self.data(hwnd);
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
        fill(&mut nid.szInfoTitle, "desk-pad");
        fill(&mut nid.szInfo, &shorten(&msg, 250));
        // SAFETY: as in `add`.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const nid) };
    }

    fn menu(&self, hwnd: HWND) {
        let status = lock(&self.shared.status).clone();
        let autostart_on = autostart::enabled(&self.paths.autostart_args);
        let checked = |on: bool| if on { MF_CHECKED } else { 0 };
        let mut items: Vec<(u32, usize, String)> = vec![
            (MF_STRING | MF_GRAYED, 0, status.describe()),
            (MF_SEPARATOR, 0, String::new()),
            (
                MF_STRING | checked(status.paused),
                ID_PAUSE,
                "Paused".into(),
            ),
            (MF_STRING, ID_RELOAD, "Reload configuration".into()),
            (MF_STRING, ID_CONFIG, "Open configuration".into()),
        ];
        if self.paths.log.is_some() {
            items.push((MF_STRING, ID_LOG, "Open log".into()));
        }
        items.extend([
            (MF_SEPARATOR, 0, String::new()),
            (
                MF_STRING | checked(autostart_on),
                ID_AUTOSTART,
                "Start with Windows".into(),
            ),
            (MF_SEPARATOR, 0, String::new()),
            (MF_STRING, ID_QUIT, "Quit".into()),
        ]);

        // SAFETY: the menu is created, shown and destroyed here; every
        // string outlives the call that copies it.
        let chosen = unsafe {
            let menu = CreatePopupMenu();
            for (flags, id, text) in &items {
                let text = wide(text);
                AppendMenuW(menu, *flags, *id, text.as_ptr());
            }
            let mut pt = POINT::default();
            GetCursorPos(&raw mut pt);
            // Without this the menu does not close when clicking elsewhere.
            SetForegroundWindow(hwnd);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                pt.x,
                pt.y,
                0,
                hwnd,
                null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            usize::try_from(chosen).unwrap_or(0)
        };

        match chosen {
            ID_PAUSE => {
                let _ = self.commands.send(Command::TogglePause);
            }
            ID_RELOAD => {
                let _ = self.commands.send(Command::Reload);
            }
            ID_CONFIG => open(&self.paths.config),
            ID_LOG => {
                if let Some(log) = &self.paths.log {
                    open(log);
                }
            }
            ID_AUTOSTART => match autostart::set(!autostart_on, &self.paths.autostart_args) {
                Ok(()) => logging::line(
                    "",
                    if autostart_on {
                        "Start with Windows: off"
                    } else {
                        "Start with Windows: on"
                    },
                ),
                Err(e) => logging::notice(&e),
            },
            ID_QUIT => stop::request(), // the input loop ends, then the tray
            _ => {}
        }
    }
}

/// `msg` cut to about `max` characters, pointing to the log if cut.
fn shorten(msg: &str, max: usize) -> String {
    if msg.chars().count() <= max {
        return msg.to_string();
    }
    let cut: String = msg.chars().take(max.saturating_sub(20)).collect();
    format!("{cut}… (see the log)")
}

/// Open a file with its program, or Notepad if it has none.
fn open(path: &std::path::Path) {
    let file = wide(&path.display().to_string());
    let verb = wide("open");
    // SAFETY: all strings are NUL-terminated and outlive the calls.
    unsafe {
        let r = ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
        if (r as isize) <= 32 {
            let notepad = wide("notepad.exe");
            let arg = wide(&format!("\"{}\"", path.display()));
            ShellExecuteW(
                null_mut(),
                verb.as_ptr(),
                notepad.as_ptr(),
                arg.as_ptr(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the `Tray` set up in `run`, which lives
    // until the message loop has ended; it is cleared on WM_DESTROY. Only
    // shared references are made: the menu's modal loop re-enters here.
    unsafe {
        let tray = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Tray;
        let Some(tray) = tray.as_ref() else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_TRAY => {
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let event = (lparam as u32) & 0xFFFF;
                if event == WM_LBUTTONUP || event == WM_RBUTTONUP {
                    tray.menu(hwnd);
                }
                0
            }
            WM_STATUS => {
                tray.update(hwnd);
                0
            }
            WM_NOTICE => {
                tray.notify(hwnd);
                0
            }
            WM_QUIT_TRAY => {
                DestroyWindow(hwnd);
                0
            }
            WM_ENDSESSION => {
                if wparam != 0 {
                    // Logging off or shutting down: release held keys first.
                    stop::request();
                    stop::wait_done(Duration::from_secs(2));
                }
                0
            }
            WM_DESTROY => {
                tray.remove(hwnd);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                0
            }
            m if m == tray.taskbar_created && m != 0 => {
                tray.add(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Show the tray icon and run its message loop until the input loop ends
/// (or Quit is chosen, which ends the input loop first).
pub fn run(shared: Arc<Shared>, commands: Sender<Command>, paths: Paths) -> Result<(), String> {
    let size = icon::small_size();
    let class = wide("desk-pad-tray");
    let title = wide("desk-pad");
    let created = wide("TaskbarCreated");
    // SAFETY: the window class and window use NUL-terminated strings that
    // outlive the calls; the `Tray` box is freed only after the message
    // loop has ended and the window is gone.
    unsafe {
        let hinst = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&raw const wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return Err("cannot create the tray icon's window".into());
        }
        let tray = Box::into_raw(Box::new(Tray {
            shared: Arc::clone(&shared),
            commands,
            paths,
            icons: LOOKS
                .iter()
                .map(|&l| (l, icon::disc(colour(l), size)))
                .collect(),
            taskbar_created: RegisterWindowMessageW(created.as_ptr()),
            added: Cell::new(false),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, tray as isize);
        (*tray).add(hwnd);
        shared.hwnd.store(hwnd as isize, Ordering::SeqCst);
        // Notices and state from before the window existed.
        PostMessageW(hwnd, WM_STATUS, 0, 0);
        PostMessageW(hwnd, WM_NOTICE, 0, 0);

        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
        shared.hwnd.store(0, Ordering::SeqCst);
        drop(Box::from_raw(tray));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_notices_are_shortened() {
        assert_eq!(shorten("short", 250), "short");
        let long = "x".repeat(600);
        let s = shorten(&long, 250);
        assert!(s.chars().count() <= 250 && s.ends_with("(see the log)"));
    }
}
