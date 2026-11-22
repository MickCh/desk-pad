//! The configuration file (TOML), parsed into a [`Layout`]. See
//! `config.toml` in the repository root for the documented default.

use crate::action::Action;
use crate::layout::{Dpad, Layout, Stick, StickRole};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

/// The default configuration, embedded into the exe.
pub const DEFAULT_CONFIG: &str = include_str!("../config.toml");

/// Highest button number accepted in `buttons`.
pub const MAX_BUTTON: u32 = 32;

/// The only profile so far.
const DEFAULT_PROFILE: &str = "default";

/// What is wrong with a configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// Not valid TOML, or an unknown option, action or key (the message
    /// names it and its line).
    Syntax(toml::de::Error),
    /// Valid TOML with an unacceptable value.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Syntax(e) => write!(f, "{}", e.to_string().trim_end()),
            ConfigError::Invalid(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A configuration file that could not be used.
#[derive(Debug)]
pub enum LoadError {
    Read(PathBuf, std::io::Error),
    Write(PathBuf, std::io::Error),
    Config(PathBuf, ConfigError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(p, e) => write!(f, "cannot read {}: {e}", p.display()),
            LoadError::Write(p, e) => write!(f, "cannot write {}: {e}", p.display()),
            LoadError::Config(p, e) => write!(f, "error in {}:\n{e}", p.display()),
        }
    }
}

impl std::error::Error for LoadError {}

/// A button number in `buttons` (as printed on the pad). Checked while
/// parsing, so a wrong one is reported with its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
struct ButtonNum(u32);

impl TryFrom<String> for ButtonNum {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        match s.trim().parse::<u32>() {
            Ok(n) if (1..=MAX_BUTTON).contains(&n) => Ok(ButtonNum(n)),
            _ => Err(format!(
                "invalid button number \"{s}\" (expected 1–{MAX_BUTTON})"
            )),
        }
    }
}

/// The file as written. Missing sections and options take the defaults of
/// [`DEFAULT_CONFIG`]'s hardware and speed settings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    pad: RawPad,
    #[serde(default)]
    sticks: RawSticks,
    #[serde(default)]
    mouse: RawMotion,
    #[serde(default)]
    scroll: RawMotion,
    #[serde(default)]
    profiles: BTreeMap<String, RawProfile>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawPad {
    debounce_ms: Option<u64>,
    repeat_ms: Option<u64>,
    detect_digital_mode: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawSticks {
    left: RawStickHw,
    right: RawStickHw,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawStickHw {
    axis_x: Option<u32>,
    axis_y: Option<u32>,
    deadzone: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawMotion {
    min_speed: Option<f32>,
    max_speed: Option<f32>,
    invert_y: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawProfile {
    buttons: HashMap<ButtonNum, Action>,
    dpad: RawDpad,
    sticks: RawRoles,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawDpad {
    up: Option<Action>,
    down: Option<Action>,
    left: Option<Action>,
    right: Option<Action>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawRoles {
    left: Option<RawRole>,
    right: Option<RawRole>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawRole {
    Mouse,
    Scroll,
    None,
}

impl RawRole {
    fn role(self) -> Option<StickRole> {
        match self {
            RawRole::Mouse => Some(StickRole::Mouse),
            RawRole::Scroll => Some(StickRole::Scroll),
            RawRole::None => None,
        }
    }
}

/// Hardware and speed defaults, for options left out of the file. They
/// equal the values in [`DEFAULT_CONFIG`] (checked by a test).
mod defaults {
    pub const DEBOUNCE_MS: u64 = 120;
    pub const REPEAT_MS: u64 = 300;
    pub const LEFT: (u32, u32, f32) = (0, 1, 0.15);
    pub const RIGHT: (u32, u32, f32) = (3, 4, 0.2);
    pub const MOUSE: (f32, f32) = (60.0, 1600.0);
    pub const SCROLL: (f32, f32) = (1.0, 25.0);
}

/// Parse and validate a configuration.
pub fn parse(text: &str) -> Result<Layout, ConfigError> {
    let raw: RawConfig = toml::from_str(text).map_err(ConfigError::Syntax)?;
    let mut profiles = raw.profiles;
    if let Some(name) = profiles.keys().find(|n| *n != DEFAULT_PROFILE) {
        return Err(ConfigError::Invalid(format!(
            "[profiles.{name}]: only [profiles.{DEFAULT_PROFILE}] is supported so far \
             (per-application profiles come in a later version)"
        )));
    }
    let profile = profiles.remove(DEFAULT_PROFILE).unwrap_or_default();

    let motion = |m: &RawMotion, (min, max): (f32, f32)| {
        (
            m.min_speed.unwrap_or(min),
            m.max_speed.unwrap_or(max),
            m.invert_y.unwrap_or(false),
        )
    };
    let mouse = motion(&raw.mouse, defaults::MOUSE);
    let scroll = motion(&raw.scroll, defaults::SCROLL);
    validate_motion("mouse", mouse)?;
    validate_motion("scroll", scroll)?;

    let stick = |side: &str, hw: &RawStickHw, (ax, ay, dz), role: Option<RawRole>| {
        let deadzone = hw.deadzone.unwrap_or(dz);
        if !(0.0..1.0).contains(&deadzone) {
            return Err(ConfigError::Invalid(format!(
                "[sticks.{side}] deadzone must be at least 0 and below 1 (got {deadzone})"
            )));
        }
        let role = role.and_then(RawRole::role);
        let (min_speed, max_speed, invert_y) = match role {
            Some(StickRole::Scroll) => scroll,
            _ => mouse,
        };
        Ok(Stick {
            role,
            axis_x: hw.axis_x.unwrap_or(ax),
            axis_y: hw.axis_y.unwrap_or(ay),
            deadzone,
            min_speed,
            max_speed,
            invert_y,
        })
    };
    let left = stick(
        "left",
        &raw.sticks.left,
        defaults::LEFT,
        profile.sticks.left,
    )?;
    let right = stick(
        "right",
        &raw.sticks.right,
        defaults::RIGHT,
        profile.sticks.right,
    )?;

    Ok(Layout {
        buttons: profile.buttons.into_iter().map(|(n, a)| (n.0, a)).collect(),
        dpad: Dpad {
            up: profile.dpad.up,
            down: profile.dpad.down,
            left: profile.dpad.left,
            right: profile.dpad.right,
        },
        repeat_ms: raw.pad.repeat_ms.unwrap_or(defaults::REPEAT_MS),
        debounce_ms: raw.pad.debounce_ms.unwrap_or(defaults::DEBOUNCE_MS),
        detect_digital_mode: raw.pad.detect_digital_mode.unwrap_or(true),
        left,
        right,
    })
}

fn validate_motion(section: &str, (min, max, _): (f32, f32, bool)) -> Result<(), ConfigError> {
    // TOML accepts nan and inf; comparisons with NaN are always false.
    if !min.is_finite() || !max.is_finite() || min < 0.0 || max < min {
        return Err(ConfigError::Invalid(format!(
            "[{section}] expected 0 <= min_speed <= max_speed, finite numbers \
             (got {min} and {max})"
        )));
    }
    Ok(())
}

/// Where the configuration lives by default: `%APPDATA%\desk-pad\desk-pad.toml`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("desk-pad").join("desk-pad.toml"))
}

pub fn load(path: &Path) -> Result<Layout, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|e| LoadError::Read(path.into(), e))?;
    parse(&text).map_err(|e| LoadError::Config(path.into(), e))
}

/// Write [`DEFAULT_CONFIG`] to `path` if there is no file yet. Returns
/// whether it was written.
pub fn create_default(path: &Path) -> Result<bool, LoadError> {
    if path.exists() {
        return Ok(false);
    }
    let write = |e| LoadError::Write(path.into(), e);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(write)?;
    }
    std::fs::write(path, DEFAULT_CONFIG).map_err(write)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn default_config_parses_with_the_documented_values() {
        let l = parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(l.buttons.len(), 10);
        assert_eq!(l.buttons[&10], Action::PadToggle);
        assert_eq!(l.buttons[&4], "key:enter".parse().unwrap());
        assert_eq!(l.dpad.right, Some("key:ctrl+tab".parse().unwrap()));
        assert_eq!(l.debounce_ms, defaults::DEBOUNCE_MS);
        assert_eq!(l.repeat_ms, defaults::REPEAT_MS);
        assert!(l.detect_digital_mode);
        assert_eq!(l.left.role, Some(StickRole::Mouse));
        assert_eq!(l.right.role, Some(StickRole::Scroll));
        assert_eq!(
            (l.left.axis_x, l.left.axis_y, l.left.deadzone),
            defaults::LEFT
        );
        assert_eq!(
            (l.right.axis_x, l.right.axis_y, l.right.deadzone),
            defaults::RIGHT
        );
        assert_eq!((l.left.min_speed, l.left.max_speed), defaults::MOUSE);
        assert_eq!((l.right.min_speed, l.right.max_speed), defaults::SCROLL);
    }

    #[test]
    fn left_out_options_take_the_defaults() {
        let l = parse("[mouse]\nmax_speed = 2000\n[profiles.default.sticks]\nleft = \"mouse\"")
            .unwrap();
        assert_eq!((l.left.min_speed, l.left.max_speed), (60.0, 2000.0));
        assert_eq!(l.right.role, None);
        assert_eq!(l.right.axis_x, 3);
        assert!(l.buttons.is_empty());
        assert_eq!(l.debounce_ms, 120);
    }

    #[test]
    fn empty_file_binds_nothing() {
        let l = parse("").unwrap();
        assert!(l.buttons.is_empty());
        assert_eq!(l.dpad.up, None);
        assert_eq!((l.left.role, l.right.role), (None, None));
    }

    #[test]
    fn errors_name_the_line() {
        let e = err("[pad]\ndebounce_ms = 100\nbounce = 3\n");
        assert!(e.contains("line 3") && e.contains("bounce"), "{e}");

        let e = err("[profiles.default.buttons]\n1 = \"mouse:left\"\n2 = \"key:ctrl+nope\"\n");
        assert!(
            e.contains("line 3") && e.contains("unknown key \"nope\""),
            "{e}"
        );

        let e = err("[profiles.default.buttons]\n13x = \"key:esc\"\n");
        assert!(
            e.contains("line 2") && e.contains("invalid button number"),
            "{e}"
        );

        let e = err("[profiles.default.dpad]\nleft = \"jump\"\n");
        assert!(e.contains("line 2") && e.contains("unknown action"), "{e}");

        let e = err("[profiles.default.sticks]\nleft = \"zoom\"\n");
        assert!(e.contains("line 2") && e.contains("zoom"), "{e}");
    }

    #[test]
    fn button_numbers_are_checked() {
        assert!(err("[profiles.default.buttons]\n0 = \"key:esc\"").contains("expected 1–32"));
        assert!(err("[profiles.default.buttons]\n33 = \"key:esc\"").contains("expected 1–32"));
        assert!(parse("[profiles.default.buttons]\n32 = \"key:esc\"").is_ok());
    }

    #[test]
    fn values_are_checked() {
        assert!(err("[sticks.left]\ndeadzone = 1.0").contains("[sticks.left] deadzone"));
        assert!(err("[sticks.right]\ndeadzone = -0.1").contains("[sticks.right] deadzone"));
        assert!(err("[mouse]\nmin_speed = 100\nmax_speed = 50").contains("[mouse]"));
        assert!(err("[scroll]\nmax_speed = nan").contains("[scroll]"));
        assert!(err("[mouse]\nmin_speed = -1").contains("[mouse]"));
    }

    #[test]
    fn other_profiles_are_not_supported_yet() {
        let e = err("[profiles.terminal.buttons]\n1 = \"key:esc\"");
        assert!(e.contains("[profiles.terminal]"), "{e}");
    }

    #[test]
    fn create_default_writes_once() {
        let dir = std::env::temp_dir().join(format!("desk-pad-test-{}", std::process::id()));
        let path = dir.join("sub").join("desk-pad.toml");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(create_default(&path).unwrap());
        std::fs::write(&path, "# mine").unwrap();
        assert!(!create_default(&path).unwrap(), "an existing file is kept");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# mine");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
