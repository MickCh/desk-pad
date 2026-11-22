//! The bindings in effect. Built in for now; read from `desk-pad.toml` in
//! stage 3 (see docs/PLAN.md).

use crate::action::Action;
use pad_core::logic::AxisParams;
use std::collections::HashMap;

/// What a stick does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickRole {
    /// Move the mouse pointer.
    Mouse,
    /// Scroll vertically and horizontally.
    Scroll,
}

#[derive(Debug, Clone)]
pub struct Stick {
    pub role: Option<StickRole>,
    /// Hardware axis indices (see `desk-pad --test`).
    pub axis_x: u32,
    pub axis_y: u32,
    /// Ignore deflections below this (0–1).
    pub deadzone: f32,
    /// Speed at a slight and at full deflection: pixels per second for the
    /// mouse, wheel notches per second for scrolling.
    pub min_speed: f32,
    pub max_speed: f32,
    /// Flip the vertical direction.
    pub invert_y: bool,
}

impl Stick {
    /// The speed curve, shared with reaper-pad's sticks.
    pub fn params(&self) -> AxisParams {
        AxisParams {
            deadzone: self.deadzone,
            min_steps_per_sec: self.min_speed,
            max_steps_per_sec: self.max_speed,
            speed: 1.0,
            flip: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Dpad {
    pub up: Option<Action>,
    pub down: Option<Action>,
    pub left: Option<Action>,
    pub right: Option<Action>,
}

#[derive(Debug, Clone)]
pub struct Layout {
    /// By the number printed on the pad (hardware index + 1).
    pub buttons: HashMap<u32, Action>,
    pub dpad: Dpad,
    /// D-pad auto-repeat interval while held.
    pub repeat_ms: u64,
    /// Ignore contact bounce of worn buttons.
    pub debounce_ms: u64,
    /// Watch for the digital mode of DragonRise pads (right stick reported as
    /// buttons 1–4) and ignore buttons 1–4 while it lasts.
    pub detect_digital_mode: bool,
    pub left: Stick,
    pub right: Stick,
}

fn action(s: &str) -> Action {
    s.parse()
        .unwrap_or_else(|e| panic!("built-in binding {s}: {e}"))
}

impl Default for Layout {
    /// Made for the Art AG-16T (button numbers as printed on it).
    fn default() -> Self {
        let buttons = [
            (1, "mouse:left"),
            (2, "mouse:right"),
            (3, "key:esc"),
            (4, "key:enter"),
            // 5 and 6 are kept for layer:nav and the window switcher (stage 4).
            (7, "key:alt+left"),
            (8, "key:alt+right"),
            (9, "key:win+tab"),
            (10, "pad:toggle"),
            (11, "mouse:middle"),
            (12, "key:win+alt+k"),
        ]
        .into_iter()
        .map(|(n, a)| (n, action(a)))
        .collect();
        Layout {
            buttons,
            dpad: Dpad {
                up: Some(action("key:up")),
                down: Some(action("key:down")),
                left: Some(action("key:ctrl+shift+tab")),
                right: Some(action("key:ctrl+tab")),
            },
            repeat_ms: 300,
            debounce_ms: 120,
            detect_digital_mode: true,
            left: Stick {
                role: Some(StickRole::Mouse),
                axis_x: 0,
                axis_y: 1,
                deadzone: 0.15,
                min_speed: 60.0,
                max_speed: 1600.0,
                invert_y: false,
            },
            right: Stick {
                role: Some(StickRole::Scroll),
                axis_x: 3,
                axis_y: 4,
                deadzone: 0.2,
                min_speed: 1.0,
                max_speed: 25.0,
                invert_y: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_builds() {
        let l = Layout::default();
        assert_eq!(l.buttons[&10], Action::PadToggle);
        assert_eq!(l.buttons.len(), 10);
    }
}
