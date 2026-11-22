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
    /// Programs this profile is for (file names, e.g. `reaper.exe`).
    #[serde(rename = "match")]
    matches: Vec<String>,
    /// Send nothing while one of them is in front.
    passthrough: bool,
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

/// A program's profile.
#[derive(Debug, Clone)]
pub struct AppProfile {
    pub name: String,
    /// Program file names (e.g. `reaper.exe`), compared without case.
    pub exes: Vec<String>,
    /// The bindings while one of the programs is in front; `None` for
    /// `passthrough`: nothing is sent then.
    pub layout: Option<Layout>,
}

/// All profiles of a configuration.
#[derive(Debug, Clone)]
pub struct Profiles {
    /// `[profiles.default]`: everywhere no other profile matches.
    pub default: Layout,
    pub apps: Vec<AppProfile>,
}

impl Profiles {
    /// The profile for the program in front (its file name), as its name
    /// and layout (`None` = passthrough).
    pub fn select(&self, exe: &str) -> (&str, Option<&Layout>) {
        self.apps
            .iter()
            .find(|p| p.exes.iter().any(|e| e.eq_ignore_ascii_case(exe)))
            .map_or((DEFAULT_PROFILE, Some(&self.default)), |p| {
                (p.name.as_str(), p.layout.as_ref())
            })
    }
}

/// Settings shared by all profiles: the pad, the sticks' hardware and the
/// speeds.
struct Base {
    repeat_ms: u64,
    debounce_ms: u64,
    detect_digital_mode: bool,
    left: (u32, u32, f32),
    right: (u32, u32, f32),
    mouse: (f32, f32, bool),
    scroll: (f32, f32, bool),
}

impl Base {
    fn new(raw: &RawConfig) -> Result<Self, ConfigError> {
        let motion = |m: &RawMotion, (min, max): (f32, f32)| {
            (
                m.min_speed.unwrap_or(min),
                m.max_speed.unwrap_or(max),
                m.invert_y.unwrap_or(false),
            )
        };
        let hw = |side: &str, h: &RawStickHw, (ax, ay, dz): (u32, u32, f32)| {
            let deadzone = h.deadzone.unwrap_or(dz);
            if !(0.0..1.0).contains(&deadzone) {
                return Err(ConfigError::Invalid(format!(
                    "[sticks.{side}] deadzone must be at least 0 and below 1 (got {deadzone})"
                )));
            }
            Ok((h.axis_x.unwrap_or(ax), h.axis_y.unwrap_or(ay), deadzone))
        };
        let base = Base {
            repeat_ms: raw.pad.repeat_ms.unwrap_or(defaults::REPEAT_MS),
            debounce_ms: raw.pad.debounce_ms.unwrap_or(defaults::DEBOUNCE_MS),
            detect_digital_mode: raw.pad.detect_digital_mode.unwrap_or(true),
            left: hw("left", &raw.sticks.left, defaults::LEFT)?,
            right: hw("right", &raw.sticks.right, defaults::RIGHT)?,
            mouse: motion(&raw.mouse, defaults::MOUSE),
            scroll: motion(&raw.scroll, defaults::SCROLL),
        };
        validate_motion("mouse", base.mouse)?;
        validate_motion("scroll", base.scroll)?;
        Ok(base)
    }

    fn stick(&self, (axis_x, axis_y, deadzone): (u32, u32, f32), role: Option<StickRole>) -> Stick {
        let (min_speed, max_speed, invert_y) = match role {
            Some(StickRole::Scroll) => self.scroll,
            _ => self.mouse,
        };
        Stick {
            role,
            axis_x,
            axis_y,
            deadzone,
            min_speed,
            max_speed,
            invert_y,
        }
    }

    /// A profile's layout: its bindings over those of `parent` (the default
    /// profile), if any.
    fn layout(&self, p: &RawProfile, parent: Option<&Layout>) -> Layout {
        let mut buttons = parent.map(|l| l.buttons.clone()).unwrap_or_default();
        buttons.extend(p.buttons.iter().map(|(n, a)| (n.0, a.clone())));
        let d = parent.map(|l| l.dpad.clone()).unwrap_or_default();
        let role = |raw: Option<RawRole>, inherited: Option<StickRole>| {
            raw.map_or(inherited, RawRole::role)
        };
        Layout {
            buttons,
            dpad: Dpad {
                up: p.dpad.up.clone().or(d.up),
                down: p.dpad.down.clone().or(d.down),
                left: p.dpad.left.clone().or(d.left),
                right: p.dpad.right.clone().or(d.right),
            },
            repeat_ms: self.repeat_ms,
            debounce_ms: self.debounce_ms,
            detect_digital_mode: self.detect_digital_mode,
            left: self.stick(
                self.left,
                role(p.sticks.left, parent.and_then(|l| l.left.role)),
            ),
            right: self.stick(
                self.right,
                role(p.sticks.right, parent.and_then(|l| l.right.role)),
            ),
        }
    }
}

impl RawProfile {
    fn has_bindings(&self) -> bool {
        let d = &self.dpad;
        !self.buttons.is_empty()
            || d.up.is_some()
            || d.down.is_some()
            || d.left.is_some()
            || d.right.is_some()
            || self.sticks.left.is_some()
            || self.sticks.right.is_some()
    }
}

/// Parse and validate a configuration.
pub fn parse(text: &str) -> Result<Profiles, ConfigError> {
    let raw: RawConfig = toml::from_str(text).map_err(ConfigError::Syntax)?;
    let base = Base::new(&raw)?;
    let invalid = |msg: String| Err(ConfigError::Invalid(msg));

    let mut profiles = raw.profiles;
    let default_raw = profiles.remove(DEFAULT_PROFILE).unwrap_or_default();
    if !default_raw.matches.is_empty() || default_raw.passthrough {
        return invalid(format!(
            "[profiles.{DEFAULT_PROFILE}] applies wherever no other profile matches: \
             it takes no match or passthrough"
        ));
    }
    let default = base.layout(&default_raw, None);

    let mut apps: Vec<AppProfile> = Vec::new();
    for (name, p) in &profiles {
        if p.matches.is_empty() {
            return invalid(format!(
                "[profiles.{name}] needs the programs it is for, e.g. match = [\"reaper.exe\"]"
            ));
        }
        if p.passthrough && p.has_bindings() {
            return invalid(format!(
                "[profiles.{name}] has passthrough = true, so it sends nothing: \
                 remove its buttons, dpad and sticks"
            ));
        }
        for exe in &p.matches {
            if let Some(other) = apps
                .iter()
                .find(|a| a.exes.iter().any(|e| e.eq_ignore_ascii_case(exe)))
            {
                return invalid(format!(
                    "\"{exe}\" is matched by both [profiles.{}] and [profiles.{name}]",
                    other.name
                ));
            }
        }
        apps.push(AppProfile {
            name: name.clone(),
            exes: p.matches.clone(),
            layout: (!p.passthrough).then(|| base.layout(p, Some(&default))),
        });
    }
    Ok(Profiles { default, apps })
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

pub fn load(path: &Path) -> Result<Profiles, LoadError> {
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

    fn layout(text: &str) -> Layout {
        parse(text).unwrap().default
    }

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn default_config_parses_with_the_documented_values() {
        let l = layout(DEFAULT_CONFIG);
        assert_eq!(l.buttons.len(), 12);
        assert_eq!(l.buttons[&5], "key:alt+v".parse().unwrap());
        assert_eq!(l.buttons[&6], "key:shift+insert".parse().unwrap());
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
        let l = layout("[mouse]\nmax_speed = 2000\n[profiles.default.sticks]\nleft = \"mouse\"");
        assert_eq!((l.left.min_speed, l.left.max_speed), (60.0, 2000.0));
        assert_eq!(l.right.role, None);
        assert_eq!(l.right.axis_x, 3);
        assert!(l.buttons.is_empty());
        assert_eq!(l.debounce_ms, 120);
    }

    #[test]
    fn empty_file_binds_nothing() {
        let l = layout("");
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
    fn reaper_is_passthrough_by_default() {
        let p = parse(DEFAULT_CONFIG).unwrap();
        let (name, layout) = p.select("REAPER.exe");
        assert_eq!(name, "reaper");
        assert!(layout.is_none(), "passthrough");
        assert_eq!(p.select("WindowsTerminal.exe").0, "default");
        assert_eq!(p.select("").0, "default", "unknown program");
    }

    #[test]
    fn app_profiles_override_the_default_bindings() {
        let p = parse(
            r#"
            [profiles.default.buttons]
            1 = "mouse:left"
            2 = "mouse:right"
            [profiles.default.dpad]
            left = "key:ctrl+shift+tab"
            up = "key:up"
            [profiles.default.sticks]
            left = "mouse"
            right = "scroll"

            [profiles.terminal]
            match = ["WindowsTerminal.exe"]
            buttons.2 = "key:esc"
            dpad.left = "key:alt+left"
            sticks.right = "none"
            "#,
        )
        .unwrap();
        let (name, l) = p.select("windowsterminal.EXE");
        let l = l.unwrap();
        assert_eq!(name, "terminal");
        assert_eq!(l.buttons[&1], "mouse:left".parse().unwrap(), "inherited");
        assert_eq!(l.buttons[&2], "key:esc".parse().unwrap(), "overridden");
        assert_eq!(l.dpad.left, Some("key:alt+left".parse().unwrap()));
        assert_eq!(l.dpad.up, Some("key:up".parse().unwrap()));
        assert_eq!(l.left.role, Some(StickRole::Mouse));
        assert_eq!(l.right.role, None, "unbound with \"none\"");
        assert_eq!(p.default.buttons[&2], "mouse:right".parse().unwrap());
    }

    #[test]
    fn profiles_are_checked() {
        let e = err("[profiles.terminal.buttons]\n1 = \"key:esc\"");
        assert!(e.contains("[profiles.terminal] needs the programs"), "{e}");
        let e = err(
            "[profiles.r]\nmatch = [\"reaper.exe\"]\npassthrough = true\nbuttons.1 = \"key:esc\"",
        );
        assert!(e.contains("passthrough"), "{e}");
        let e = err("[profiles.a]\nmatch = [\"x.exe\"]\n[profiles.b]\nmatch = [\"X.EXE\"]");
        assert!(e.contains("both [profiles.a] and [profiles.b]"), "{e}");
        let e = err("[profiles.default]\nmatch = [\"x.exe\"]");
        assert!(e.contains("[profiles.default]"), "{e}");
        let e = err("[profiles.x]\nmatch = [\"x.exe\"]\ntitle = \"y\"");
        assert!(e.contains("title"), "unknown option: {e}");
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
