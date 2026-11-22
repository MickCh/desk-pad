//! The bindings and settings in effect, as read from the configuration
//! file by [`crate::config`].

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

impl Default for Layout {
    /// The documented default configuration (`config.toml`, embedded).
    fn default() -> Self {
        crate::config::parse(crate::config::DEFAULT_CONFIG)
            .unwrap_or_else(|e| panic!("built-in config.toml: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_builds() {
        let l = Layout::default();
        assert_eq!(l.buttons[&10], Action::PadToggle);
        assert_eq!(l.buttons.len(), 12);
    }
}
