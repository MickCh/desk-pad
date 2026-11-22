//! The input loop: pad → router → `SendInput`, with the profile following
//! the program in front, configuration reloads, and the state the tray
//! icon shows.

use crate::config::{self, Profiles};
use crate::logging::{self, AppLog};
use crate::router::{Output, Router};
use crate::stop;
use crate::watch::Watcher;
use crate::win;
use crate::win::foreground::{Foreground, foreground};
use crate::win::send::SendInputOutput;
use crate::win::tray::{Command, Link};
use pad_core::gamepad::{Gamepad, PadEvent};
use pad_core::input::Log;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// Input loop period: ~250 Hz, smooth enough for the pointer.
const PERIOD: Duration = Duration::from_millis(4);

/// How often the program in front is checked (to choose the profile).
const FOREGROUND_CHECK: Duration = Duration::from_millis(100);

/// Command-line options.
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// Button test: report input, send nothing.
    pub test: bool,
    /// Run in a console instead of the tray.
    pub console: bool,
    /// Stop after this long.
    pub limit: Option<Duration>,
    /// Configuration file instead of the default one.
    pub config: Option<PathBuf>,
}

/// What the tray icon shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub pad: bool,
    pub paused: bool,
    pub digital: bool,
    pub passthrough: bool,
    pub profile: String,
}

/// The state as one of the tray icon's looks, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    NoPad,
    Passthrough,
    Paused,
    Digital,
    Active,
}

impl Status {
    pub fn look(&self) -> Look {
        if !self.pad {
            Look::NoPad
        } else if self.passthrough {
            Look::Passthrough
        } else if self.paused {
            Look::Paused
        } else if self.digital {
            Look::Digital
        } else {
            Look::Active
        }
    }

    /// One line for the tooltip and the menu.
    pub fn describe(&self) -> String {
        let profile = if self.profile.is_empty() {
            "default"
        } else {
            &self.profile
        };
        match self.look() {
            Look::NoPad => "Waiting for a gamepad".into(),
            Look::Passthrough => format!("Profile {profile}: passthrough, nothing is sent"),
            Look::Paused => "Paused".into(),
            Look::Digital => "Gamepad in digital mode (LED green): press Analog".into(),
            Look::Active => format!("Active, profile {profile}"),
        }
    }
}

/// The profile in effect, following the program in front.
#[derive(Default)]
struct ActiveProfile {
    /// Name of the profile in effect; empty until the first choice.
    name: String,
}

impl ActiveProfile {
    /// Switch to the profile of `exe` if it differs, once no pad button is
    /// held (a drag or a held key ends where it started).
    fn follow(
        &mut self,
        exe: &str,
        profiles: &Profiles,
        router: &mut Router,
        out: &mut dyn Output,
    ) {
        let (name, layout) = profiles.select(exe);
        if name == self.name || !router.idle() {
            return;
        }
        match layout {
            Some(l) => {
                router.set_layout(l.clone(), out);
                router.set_passthrough(false, out);
            }
            None => router.set_passthrough(true, out),
        }
        let first = self.name.is_empty();
        self.name = name.to_string();
        if !(first && name == "default") {
            let note = if layout.is_none() {
                " (passthrough: nothing is sent)"
            } else {
                ""
            };
            out.log(&format!("Profile: {name}{note}"));
        }
    }
}

/// Read the configuration; on an error, say so, with `then` (what happens
/// instead).
fn load(path: &Path, log: &mut dyn Log, then: &str) -> Option<Profiles> {
    config::load(path)
        .map_err(|e| log.notice(&format!("{e}\n{then}")))
        .ok()
}

/// Run until [`stop::requested`] (or `opts.limit`). Everything held is
/// released before returning.
pub fn run(opts: &Options, path: &Path, link: Option<&Link>) -> Result<(), String> {
    let end = opts.limit.map(|d| Instant::now() + d);
    let mut out = SendInputOutput::new(AppLog);
    let created = config::create_default(path).map_err(|e| e.to_string())?;
    out.log(&format!(
        "Configuration: {}{}",
        path.display(),
        if created {
            " (created with the defaults)"
        } else {
            ""
        }
    ));
    let mut profiles = load(
        path,
        &mut out,
        "Using the built-in defaults until it is fixed.",
    )
    .unwrap_or_else(|| config::parse(config::DEFAULT_CONFIG).expect("built-in config.toml"));
    let mut watcher = Watcher::new(path.to_path_buf(), Instant::now());

    let _timer = win::TimerResolution::new();
    let mut gamepad = Gamepad::new().map_err(|e| format!("SDL: {e}"))?;
    let mut router = Router::new(profiles.default.clone(), Instant::now());
    let mut active = ActiveProfile::default();
    router.set_test_mode(opts.test, &mut out);
    if opts.test {
        out.log("Button test: press buttons, move the sticks, switch windows. Ctrl+C ends.");
    }
    match gamepad.open_first() {
        Some(device) => {
            out.log(&format!(
                "Gamepad: {}",
                gamepad.describe().unwrap_or_default()
            ));
            router.connected(device);
        }
        None => out.log("Waiting for a gamepad…"),
    }

    let mut front: Option<Foreground> = None;
    let mut next_front_check = Instant::now();
    let mut shown = None;
    while !stop::requested() && end.is_none_or(|e| Instant::now() < e) {
        let now = Instant::now();
        gamepad.poll(|ev| match ev {
            PadEvent::Connected { name, device } => {
                out.log(&format!("Gamepad connected: {name}"));
                router.connected(device);
            }
            PadEvent::Disconnected => {
                out.log("Gamepad disconnected — waiting…");
                router.disconnected(now, &mut out);
            }
            PadEvent::Input(input) => router.event(input, now, &mut out),
        });
        router.tick(now, |axis| gamepad.axis(axis), &mut out);

        let mut reload = watcher.poll(now);
        for cmd in link.into_iter().flat_map(|l| l.commands.try_iter()) {
            match cmd {
                Command::TogglePause => router.toggle_pause(&mut out),
                Command::Reload => reload = true,
            }
        }
        if reload && let Some(p) = load(path, &mut out, "The previous settings stay active.") {
            profiles = p;
            router.set_layout(profiles.default.clone(), &mut out);
            router.set_passthrough(false, &mut out);
            active = ActiveProfile::default(); // chosen again below
            out.log("Configuration reloaded.");
        }

        if now >= next_front_check {
            next_front_check = now + FOREGROUND_CHECK;
            let current = foreground();
            if opts.test
                && current != front
                && let Some(f) = &current
            {
                out.log(&format!("window: {} — \"{}\"", f.exe, f.title));
            }
            front = current;
        }
        let exe = front.as_ref().map_or("", |f| f.exe.as_str());
        active.follow(exe, &profiles, &mut router, &mut out);

        if let Some(link) = link {
            let status = Status {
                pad: router.has_pad(),
                paused: router.paused(),
                digital: router.digital(),
                passthrough: router.passthrough(),
                profile: active.name.clone(),
            };
            if shown.as_ref() != Some(&status) {
                link.shared.set_status(status.clone());
                shown = Some(status);
            }
        }
        thread::sleep(PERIOD);
    }

    router.release_all(&mut out);
    logging::line("", "Stopped.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(pad: bool, paused: bool, digital: bool, passthrough: bool) -> Status {
        Status {
            pad,
            paused,
            digital,
            passthrough,
            profile: "terminal".into(),
        }
    }

    #[test]
    fn the_most_important_state_shows() {
        assert_eq!(status(false, true, true, true).look(), Look::NoPad);
        assert_eq!(status(true, true, true, true).look(), Look::Passthrough);
        assert_eq!(status(true, true, true, false).look(), Look::Paused);
        assert_eq!(status(true, false, true, false).look(), Look::Digital);
        assert_eq!(status(true, false, false, false).look(), Look::Active);
        assert_eq!(
            status(true, false, false, false).describe(),
            "Active, profile terminal"
        );
        assert_eq!(
            Status {
                pad: true,
                ..Status::default()
            }
            .describe(),
            "Active, profile default"
        );
    }
}
