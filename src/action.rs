//! What a button or D-pad direction does.

use crate::keys::{Chord, ChordError};
use serde::Deserialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// The "back" side button (X1).
    Back,
    /// The "forward" side button (X2).
    Forward,
}

impl MouseButton {
    const NAMES: &[(&str, MouseButton)] = &[
        ("left", MouseButton::Left),
        ("right", MouseButton::Right),
        ("middle", MouseButton::Middle),
        ("back", MouseButton::Back),
        ("forward", MouseButton::Forward),
    ];

    fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|&&(_, b)| b == self)
            .map_or("?", |(n, _)| n)
    }
}

/// A binding. Keys and mouse buttons stay down as long as the pad's button.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum Action {
    /// `key:ctrl+tab`
    Key(Chord),
    /// `mouse:left`
    Mouse(MouseButton),
    /// `pad:toggle`: pause or resume the pad.
    PadToggle,
    /// `text:…`: type the text (on press; `\n` is Enter).
    Text(String),
    /// `run:program args`: start a program, open a file, folder or URL.
    Run(String),
    /// `layer:<name>`: while held, the layer's bindings apply.
    Layer(String),
    /// `none`: nothing, also where a profile or layer would inherit a
    /// binding.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    Unknown(String),
    Key(String, ChordError),
    MouseButton(String),
    Empty(String),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::Unknown(s) => write!(
                f,
                "unknown action \"{s}\" (expected key:<keys>, mouse:<button>, text:<text>, run:<command>, layer:<name>, pad:toggle or none)"
            ),
            ActionError::Key(s, e) => write!(f, "\"{s}\": {e}"),
            ActionError::Empty(s) => write!(f, "\"{s}\": nothing after the colon"),
            ActionError::MouseButton(s) => write!(
                f,
                "unknown mouse button in \"{s}\" (expected one of: {})",
                MouseButton::NAMES
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for ActionError {}

impl FromStr for Action {
    type Err = ActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Text is taken as written (spaces, line ends); everything else
        // trimmed.
        if let Some(text) = s.trim_start().strip_prefix("text:") {
            return if text.is_empty() {
                Err(ActionError::Empty(s.into()))
            } else {
                Ok(Action::Text(text.into()))
            };
        }
        let s = s.trim();
        for (prefix, make) in [
            ("run:", Action::Run as fn(String) -> Action),
            ("layer:", Action::Layer),
        ] {
            if let Some(rest) = s.strip_prefix(prefix) {
                let rest = rest.trim();
                return if rest.is_empty() {
                    Err(ActionError::Empty(s.into()))
                } else {
                    Ok(make(rest.into()))
                };
            }
        }
        if let Some(keys) = s.strip_prefix("key:") {
            return keys
                .parse()
                .map(Action::Key)
                .map_err(|e| ActionError::Key(s.into(), e));
        }
        if let Some(button) = s.strip_prefix("mouse:") {
            return MouseButton::NAMES
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(button.trim()))
                .map(|&(_, b)| Action::Mouse(b))
                .ok_or_else(|| ActionError::MouseButton(s.into()));
        }
        match s {
            "pad:toggle" => Ok(Action::PadToggle),
            "none" => Ok(Action::None),
            _ => Err(ActionError::Unknown(s.into())),
        }
    }
}

impl TryFrom<String> for Action {
    type Error = ActionError;

    fn try_from(s: String) -> Result<Self, ActionError> {
        s.parse()
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Key(chord) => write!(f, "key:{chord}"),
            Action::Mouse(b) => write!(f, "mouse:{}", b.name()),
            Action::PadToggle => f.write_str("pad:toggle"),
            Action::Text(t) => write!(f, "text:{}", t.escape_debug()),
            Action::Run(c) => write!(f, "run:{c}"),
            Action::Layer(n) => write!(f, "layer:{n}"),
            Action::None => f.write_str("none"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_back() {
        for s in [
            "key:ctrl+tab",
            "key:alt+v",
            "mouse:left",
            "mouse:forward",
            "pad:toggle",
            "text:/clear",
            "run:notepad.exe",
            "layer:windows",
            "none",
        ] {
            assert_eq!(s.parse::<Action>().unwrap().to_string(), s);
        }
    }

    #[test]
    fn text_is_kept_as_written() {
        assert_eq!(
            "text:git status\n".parse::<Action>().unwrap(),
            Action::Text("git status\n".into())
        );
        assert_eq!(
            "run:  wt.exe -w 0 nt ".parse::<Action>().unwrap(),
            Action::Run("wt.exe -w 0 nt".into())
        );
        assert!(matches!(
            "text:".parse::<Action>(),
            Err(ActionError::Empty(_))
        ));
        assert!(matches!(
            "layer: ".parse::<Action>(),
            Err(ActionError::Empty(_))
        ));
    }

    #[test]
    fn errors() {
        assert!(matches!(
            "jump".parse::<Action>(),
            Err(ActionError::Unknown(_))
        ));
        assert!(matches!(
            "key:ctrl+nope".parse::<Action>(),
            Err(ActionError::Key(..))
        ));
        assert!(matches!(
            "mouse:wheel".parse::<Action>(),
            Err(ActionError::MouseButton(_))
        ));
        let msg = "mouse:wheel".parse::<Action>().unwrap_err().to_string();
        assert!(msg.contains("left, right, middle"), "{msg}");
    }
}
