//! Key names and chords such as `ctrl+shift+tab`, as Windows virtual-key
//! codes. Plain data, so it is tested on any platform.

use std::fmt;
use std::str::FromStr;

/// A Windows virtual-key code (`VK_*`).
pub type Vk = u16;

/// Modifier keys, in the order they are pressed.
const MODIFIERS: &[(&str, Vk)] = &[
    ("ctrl", 0x11),  // VK_CONTROL
    ("shift", 0x10), // VK_SHIFT
    ("alt", 0x12),   // VK_MENU
    ("win", 0x5B),   // VK_LWIN
];

/// Named keys other than letters, digits and F1–F24.
const NAMED: &[(&str, Vk)] = &[
    ("backspace", 0x08),
    ("tab", 0x09),
    ("enter", 0x0D),
    ("pause", 0x13),
    ("capslock", 0x14),
    ("esc", 0x1B),
    ("space", 0x20),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("end", 0x23),
    ("home", 0x24),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("printscreen", 0x2C),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("menu", 0x5D), // the context-menu key
    ("browser_back", 0xA6),
    ("browser_forward", 0xA7),
    ("browser_refresh", 0xA8),
    ("volume_mute", 0xAD),
    ("volume_down", 0xAE),
    ("volume_up", 0xAF),
    ("media_next", 0xB0),
    ("media_prev", 0xB1),
    ("media_stop", 0xB2),
    ("media_play_pause", 0xB3),
    (";", 0xBA),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
];

/// Other spellings people write.
const ALIASES: &[(&str, &str)] = &[
    ("control", "ctrl"),
    ("escape", "esc"),
    ("return", "enter"),
    ("del", "delete"),
    ("ins", "insert"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
    ("windows", "win"),
    ("plus", "="),
    ("minus", "-"),
];

/// Virtual-key code for one key name (case-insensitive), or `None`.
pub fn vk(name: &str) -> Option<Vk> {
    let lower = name.to_ascii_lowercase();
    let name = ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map_or(lower.as_str(), |(_, real)| real);
    if let [c] = name.as_bytes()
        && (c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Some(Vk::from(c.to_ascii_uppercase())); // VK_A.. = 'A'.., VK_0.. = '0'..
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<Vk>().ok()) {
        return (1..=24).contains(&n).then_some(0x70 + n - 1); // VK_F1..VK_F24
    }
    MODIFIERS
        .iter()
        .chain(NAMED)
        .find(|(n, _)| *n == name)
        .map(|&(_, vk)| vk)
}

/// Whether `vk` is one of the modifiers (ctrl, shift, alt, win).
pub fn is_modifier(vk: Vk) -> bool {
    MODIFIERS.iter().any(|&(_, m)| m == vk)
}

/// Name of a virtual-key code, for messages.
pub fn name(vk: Vk) -> String {
    if let Some((n, _)) = MODIFIERS.iter().chain(NAMED).find(|&&(_, v)| v == vk) {
        return (*n).to_string();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(u8::try_from(vk).unwrap_or(b'?'))
            .to_ascii_lowercase()
            .to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x70 + 1),
        _ => format!("vk{vk:#04x}"),
    }
}

/// Keys pressed together: modifiers first, then the key. Released in the
/// opposite order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: Vec<Vk>,
    pub key: Vk,
}

impl Chord {
    /// All keys in press order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = Vk> + '_ {
        self.modifiers.iter().copied().chain([self.key])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordError {
    Empty,
    UnknownKey(String),
    /// Something other than a modifier before the last `+`.
    NotAModifier(String),
}

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordError::Empty => write!(f, "no key given (e.g. key:ctrl+tab)"),
            ChordError::UnknownKey(k) => write!(
                f,
                "unknown key \"{k}\" (letters, digits, f1–f24, {})",
                MODIFIERS
                    .iter()
                    .chain(NAMED)
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ChordError::NotAModifier(k) => write!(
                f,
                "\"{k}\" is not a modifier: only ctrl, shift, alt and win can come before the last +"
            ),
        }
    }
}

impl std::error::Error for ChordError {}

impl FromStr for Chord {
    type Err = ChordError;

    /// `ctrl+shift+tab`, `alt+v`, `f5`, `win`. A lone `+` key is written
    /// `plus` (it is the `=` key).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let Some((&last, mods)) = parts.split_last() else {
            return Err(ChordError::Empty);
        };
        if last.is_empty() {
            return Err(ChordError::Empty);
        }
        let key = vk(last).ok_or_else(|| ChordError::UnknownKey(last.into()))?;
        let mut modifiers = Vec::new();
        for &m in mods {
            let code = vk(m).ok_or_else(|| ChordError::UnknownKey(m.into()))?;
            if !is_modifier(code) {
                return Err(ChordError::NotAModifier(m.into()));
            }
            if !modifiers.contains(&code) {
                modifiers.push(code);
            }
        }
        Ok(Chord { modifiers, key })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.keys().map(name).collect();
        f.write_str(&names.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        s.parse().unwrap()
    }

    #[test]
    fn key_names() {
        assert_eq!(vk("a"), Some(0x41));
        assert_eq!(vk("V"), Some(0x56));
        assert_eq!(vk("0"), Some(0x30));
        assert_eq!(vk("f1"), Some(0x70));
        assert_eq!(vk("F24"), Some(0x87));
        assert_eq!(vk("f25"), None);
        assert_eq!(vk("Tab"), Some(0x09));
        assert_eq!(vk("escape"), Some(0x1B));
        assert_eq!(vk("Insert"), Some(0x2D));
        assert_eq!(vk("nope"), None);
    }

    #[test]
    fn chords() {
        assert_eq!(
            chord("ctrl+shift+tab"),
            Chord {
                modifiers: vec![0x11, 0x10],
                key: 0x09
            }
        );
        assert_eq!(
            chord(" Alt + V "),
            Chord {
                modifiers: vec![0x12],
                key: 0x56
            }
        );
        assert_eq!(
            chord("shift+insert"),
            Chord {
                modifiers: vec![0x10],
                key: 0x2D
            }
        );
        assert_eq!(
            chord("win"),
            Chord {
                modifiers: vec![],
                key: 0x5B
            }
        );
        assert_eq!(chord("ctrl+ctrl+a").modifiers, vec![0x11], "no duplicates");
    }

    #[test]
    fn chord_errors() {
        assert_eq!("".parse::<Chord>(), Err(ChordError::Empty));
        assert_eq!("ctrl+".parse::<Chord>(), Err(ChordError::Empty));
        assert_eq!(
            "ctrl+bogus".parse::<Chord>(),
            Err(ChordError::UnknownKey("bogus".into()))
        );
        assert_eq!(
            "a+b".parse::<Chord>(),
            Err(ChordError::NotAModifier("a".into()))
        );
    }

    #[test]
    fn chords_print_back() {
        for s in ["ctrl+shift+tab", "alt+v", "win+alt+k", "f5", "ctrl+="] {
            assert_eq!(chord(s).to_string(), s);
        }
    }
}
