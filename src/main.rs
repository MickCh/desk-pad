//! desk-pad: use a USB gamepad as keyboard and mouse on the Windows desktop.
//!
//! Layout: [`router`] decides what the input means (plain Rust, unit-tested
//! with a mock output), using [`action`], [`keys`] and [`layout`]. The pad
//! is read through `pad_core::gamepad` (SDL2, shared with reaper-pad);
//! [`win`] holds everything that calls the Win32 API.

mod action;
mod keys;
mod layout;
mod router;
mod win;

use layout::Layout;
use pad_core::gamepad::{Gamepad, PadEvent};
use pad_core::input::Log;
use router::Router;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};
use win::foreground::{Foreground, foreground};
use win::send::SendInputOutput;

/// Input loop period: ~250 Hz, smooth enough for the pointer.
const PERIOD: Duration = Duration::from_millis(4);

/// How often `--test` checks which window is in front.
const FOREGROUND_CHECK: Duration = Duration::from_millis(200);

const HELP: &str = "\
desk-pad: use a USB gamepad as keyboard and mouse.

Usage: desk-pad [--test] [--seconds N]

  --test        Button test: print buttons (with timing), axes and D-pad as
                they are used, and the program and title of the window in
                front. Sends nothing.
  --seconds N   Stop after N seconds (e.g. to run the test from a script).

Stop with Ctrl+C.";

/// Log to the console, with the time since start.
struct ConsoleLog {
    start: Instant,
}

impl Log for ConsoleLog {
    fn log(&mut self, msg: &str) {
        println!("[{:8.3}] {msg}", self.start.elapsed().as_secs_f32());
    }
    fn notice(&mut self, msg: &str) {
        println!("[{:8.3}] ! {msg}", self.start.elapsed().as_secs_f32());
    }
}

fn main() -> ExitCode {
    let mut test = false;
    let mut limit = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--test" => test = true,
            "--seconds" => match args.next().and_then(|n| n.parse::<f32>().ok()) {
                Some(n) if n > 0.0 => limit = Some(Duration::from_secs_f32(n)),
                _ => {
                    eprintln!(
                        "--seconds needs a positive number

{HELP}"
                    );
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown option {other}\n\n{HELP}");
                return ExitCode::from(2);
            }
        }
    }
    match run(test, limit) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("desk-pad: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(test: bool, limit: Option<Duration>) -> Result<(), String> {
    let end = limit.map(|d| Instant::now() + d);
    let mut out = SendInputOutput::new(ConsoleLog {
        start: Instant::now(),
    });
    if !win::console::install() {
        out.notice("Could not catch Ctrl+C: keys held at exit may stay pressed.");
    }
    let _timer = win::TimerResolution::new();
    let mut gamepad = Gamepad::new().map_err(|e| format!("SDL: {e}"))?;
    let mut router = Router::new(Layout::default(), Instant::now());
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

        if test && now >= next_front_check {
            next_front_check = now + FOREGROUND_CHECK;
            let current = foreground();
            if current != front {
                if let Some(f) = &current {
                    out.log(&format!("window: {} — \"{}\"", f.exe, f.title));
                }
                front = current;
            }
        }
        thread::sleep(PERIOD);
    }

    router.release_all(&mut out);
    out.log("Stopped.");
    win::console::done();
    Ok(())
}
