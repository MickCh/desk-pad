//! The router's [`Output`] through `SendInput`.

use crate::action::MouseButton;
use crate::keys::Vk;
use crate::router::Output;
use pad_core::input::Log;
use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC_EX, MOUSE_EVENT_FLAGS, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyW, SendInput,
};

/// Tag on every event desk-pad injects (`dwExtraInfo`), to recognise its
/// own input later (e.g. in a keyboard hook).
pub const EXTRA_INFO: usize = 0xDE5C_9AD0;

/// Sends input to whichever window has focus, and logs through `log`.
pub struct SendInputOutput<L: Log> {
    pub log: L,
    /// Whether the last `SendInput` was refused, so it is reported once
    /// per change, not on every event.
    blocked: bool,
    /// Keys and mouse buttons sent down and not yet up: released on drop,
    /// so not even a panic leaves them pressed.
    down_keys: Vec<Vk>,
    down_mouse: Vec<MouseButton>,
}

impl<L: Log> Drop for SendInputOutput<L> {
    fn drop(&mut self) {
        for b in std::mem::take(&mut self.down_mouse) {
            self.mouse_button(b, false);
        }
        for k in std::mem::take(&mut self.down_keys).into_iter().rev() {
            self.key(k, false);
        }
    }
}

fn track<T: PartialEq>(list: &mut Vec<T>, item: T, down: bool) {
    if down {
        if !list.contains(&item) {
            list.push(item);
        }
    } else {
        list.retain(|i| *i != item);
    }
}

impl<L: Log> SendInputOutput<L> {
    pub fn new(log: L) -> Self {
        SendInputOutput {
            log,
            blocked: false,
            down_keys: Vec::new(),
            down_mouse: Vec::new(),
        }
    }

    fn send(&mut self, input: INPUT) {
        // SAFETY: one valid INPUT, with its size as the API requires.
        let sent = unsafe { SendInput(1, &raw const input, size_of::<INPUT>() as i32) };
        let blocked = sent == 0;
        if blocked != self.blocked {
            self.blocked = blocked;
            if blocked {
                self.log.notice(
                    "Input blocked: the active window probably belongs to a program \
                     running as administrator (run desk-pad as administrator too).",
                );
            } else {
                self.log.log("Input accepted again.");
            }
        }
    }

    fn mouse(&mut self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) {
        self.send(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    // Wheel deltas are signed but the field is not.
                    mouseData: data.cast_unsigned(),
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        });
    }
}

impl<L: Log> Log for SendInputOutput<L> {
    fn log(&mut self, msg: &str) {
        self.log.log(msg);
    }
    fn notice(&mut self, msg: &str) {
        self.log.notice(msg);
    }
}

impl<L: Log> Output for SendInputOutput<L> {
    fn key(&mut self, vk: Vk, down: bool) {
        track(&mut self.down_keys, vk, down);
        // Scan code with the E0 prefix in the high byte for extended keys
        // (arrows, Insert, Home, Win, …); apps reading either see the key.
        // SAFETY: plain call without pointers.
        let sc = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC_EX) };
        let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
        if sc & 0xFF00 == 0xE000 || sc & 0xFF00 == 0xE100 {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        self.send(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: (sc & 0xFF) as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        });
    }

    fn mouse_button(&mut self, button: MouseButton, down: bool) {
        track(&mut self.down_mouse, button, down);
        let (flags, data) = match (button, down) {
            (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            // XBUTTON1 = 1 (back), XBUTTON2 = 2 (forward).
            (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, 1),
            (MouseButton::Back, false) => (MOUSEEVENTF_XUP, 1),
            (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, 2),
            (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, 2),
        };
        self.mouse(flags, 0, 0, data);
    }

    fn mouse_move(&mut self, dx: i32, dy: i32) {
        self.mouse(MOUSEEVENTF_MOVE, dx, dy, 0);
    }

    fn wheel(&mut self, vertical: i32, horizontal: i32) {
        if vertical != 0 {
            self.mouse(MOUSEEVENTF_WHEEL, 0, 0, vertical);
        }
        if horizontal != 0 {
            self.mouse(MOUSEEVENTF_HWHEEL, 0, 0, horizontal);
        }
    }
}
