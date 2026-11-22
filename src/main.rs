//! desk-pad: use a USB gamepad as keyboard and mouse on the Windows desktop.
//!
//! Layout: [`router`] decides what the input means (plain Rust, unit-tested
//! with a mock output), using [`action`], [`keys`] and [`layout`]. The pad
//! is read through `pad_core::gamepad` (SDL2, shared with reaper-pad);
//! [`win`] holds everything that calls the Win32 API.

mod action;
mod config;
mod keys;
mod layout;
mod router;
mod watch;
mod win;

use config::Profiles;
use pad_core::gamepad::{Gamepad, PadEvent};
use pad_core::input::Log;
use router::{Output, Router};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};
use watch::Watcher;
use win::foreground::{Foreground, foreground};
use win::send::SendInputOutput;

/// Input loop period: ~250 Hz, smooth enough for the pointer.
const PERIOD: Duration = Duration::from_millis(4);

/// How often the program in front is checked (to choose the profile).
const FOREGROUND_CHECK: Duration = Duration::from_millis(100);

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
            out.log(&format!(
                "Profile: {name}{}",
                if layout.is_none() {
                    " (passthrough: nothing is sent)"
                } else {
                    ""
                }
            ));
        }
    }
}

const HELP: &str = "\
desk-pad: use a USB gamepad as keyboard and mouse.

Usage: desk-pad [--test] [--seconds N] [--config PATH]

  --test          Button test: print buttons (with timing), axes and D-pad
                  as they are used, and the program and title of the window
                  in front. Sends nothing.
  --seconds N     Stop after N seconds (e.g. to run the test from a script).
  --config PATH   Configuration file (default:
                  %APPDATA%\\desk-pad\\desk-pad.toml, created on first start).
                  Reloaded when saved.

Stop with Ctrl+C.";

/// Log to the console, with the time since start.
struct ConsoleLog {
    start: Instant,
}

impl Log for ConsoleLog {
    fn log(&mut self, msg: &str) {
        self.write("", msg);
    }
    fn notice(&mut self, msg: &str) {
        self.write("! ", msg);
    }
}

impl ConsoleLog {
    /// Errors are ignored: `println!` would panic when the output is a
    /// closed pipe, and a panic must not skip releasing held keys.
    fn write(&self, mark: &str, msg: &str) {
        let t = self.start.elapsed().as_secs_f32();
        let _ = writeln!(std::io::stdout(), "[{t:8.3}] {mark}{msg}");
    }
}

/// Command-line options.
struct Options {
    test: bool,
    limit: Option<Duration>,
    config: Option<PathBuf>,
}

/// `Err` holds the message for an invalid command line; `Ok(None)` means
/// help was printed.
fn options() -> Result<Option<Options>, String> {
    let mut o = Options {
        test: false,
        limit: None,
        config: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--test" => o.test = true,
            "--seconds" => match args.next().and_then(|n| n.parse::<f32>().ok()) {
                Some(n) if n > 0.0 && n.is_finite() => {
                    o.limit = Some(Duration::from_secs_f32(n));
                }
                _ => return Err("--seconds needs a positive number".into()),
            },
            "--config" => match args.next() {
                Some(p) => o.config = Some(p.into()),
                None => return Err("--config needs a file name".into()),
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Some(o))
}

fn main() -> ExitCode {
    let opts = match options() {
        Ok(Some(o)) => o,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "{e}

{HELP}"
            );
            return ExitCode::from(2);
        }
    };
    match run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("desk-pad: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(opts: &Options) -> Result<(), String> {
    let test = opts.test;
    let end = opts.limit.map(|d| Instant::now() + d);
    let path = match &opts.config {
        Some(p) => p.clone(),
        None => config::default_path().ok_or("APPDATA is not set: use --config PATH")?,
    };
    let created = config::create_default(&path).map_err(|e| e.to_string())?;
    let mut profiles = config::load(&path).map_err(|e| e.to_string())?;
    let mut watcher = Watcher::new(path.clone(), Instant::now());

    let mut out = SendInputOutput::new(ConsoleLog {
        start: Instant::now(),
    });
    if !win::console::install() {
        out.notice("Could not catch Ctrl+C: keys held at exit may stay pressed.");
    }
    let _timer = win::TimerResolution::new();
    let mut gamepad = Gamepad::new().map_err(|e| format!("SDL: {e}"))?;
    out.log(&format!(
        "Configuration: {}{}",
        path.display(),
        if created {
            " (created with the defaults)"
        } else {
            ""
        }
    ));
    let mut router = Router::new(profiles.default.clone(), Instant::now());
    let mut active = ActiveProfile::default();
    router.set_test_mode(test, &mut out);
    if test {
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
    while !win::console::stop_requested() && end.is_none_or(|e| Instant::now() < e) {
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

        if watcher.poll(now) {
            match config::load(&path) {
                Ok(p) => {
                    profiles = p;
                    router.set_layout(profiles.default.clone(), &mut out);
                    router.set_passthrough(false, &mut out);
                    active = ActiveProfile::default(); // chosen again below
                    out.log("Configuration reloaded.");
                }
                Err(e) => out.notice(&format!(
                    "{e}
The previous settings stay active."
                )),
            }
        }

        if now >= next_front_check {
            next_front_check = now + FOREGROUND_CHECK;
            let current = foreground();
            if test
                && current != front
                && let Some(f) = &current
            {
                out.log(&format!("window: {} — \"{}\"", f.exe, f.title));
            }
            front = current;
        }
        let exe = front.as_ref().map_or("", |f| f.exe.as_str());
        active.follow(exe, &profiles, &mut router, &mut out);
        thread::sleep(PERIOD);
    }

    router.release_all(&mut out);
    out.log("Stopped.");
    win::console::done();
    Ok(())
}
