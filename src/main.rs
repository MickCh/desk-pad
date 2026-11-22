//! desk-pad: use a USB gamepad as keyboard and mouse on the Windows desktop.
//!
//! Layout: [`app`] runs the input loop; [`router`] decides what the input
//! means (plain Rust, unit-tested with a mock output), using [`action`],
//! [`keys`], [`layout`] and [`config`]. The pad is read through
//! `pad_core::gamepad` (SDL2, shared with reaper-pad); [`win`] holds
//! everything that calls the Win32 API, including the tray icon.
//!
//! A Windows (GUI) program: normally it lives in the tray, without a
//! console; `--test` and `--console` open one.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod action;
mod app;
mod config;
mod keys;
mod layout;
mod logging;
mod router;
mod stop;
mod watch;
mod win;

use app::Options;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;
use win::console;
use win::tray::{Link, Paths, Shared};

const HELP: &str = "\
desk-pad: use a USB gamepad as keyboard and mouse.

Usage: desk-pad [--test | --console] [--seconds N] [--config PATH]

  (none)          Run in the background with an icon in the notification
                  area (tray); its menu pauses, reloads, opens the
                  configuration and the log, and quits.
  --test          Button test in a console: print buttons (with timing),
                  axes and D-pad as they are used, and the program and
                  title of the window in front. Sends nothing.
  --console       Run in a console instead of the tray (stop with Ctrl+C).
  --seconds N     Stop after N seconds.
  --config PATH   Configuration file (default:
                  %APPDATA%\\desk-pad\\desk-pad.toml, created on first start).
                  Reloaded when saved.";

enum Parsed {
    Run(Options),
    Help,
}

fn options() -> Result<Parsed, String> {
    let mut o = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--test" => o.test = true,
            "--console" => o.console = true,
            "--seconds" => match args.next().and_then(|n| n.parse::<f32>().ok()) {
                Some(n) if n > 0.0 && n.is_finite() => o.limit = Some(Duration::from_secs_f32(n)),
                _ => return Err("--seconds needs a positive number".into()),
            },
            "--config" => match args.next() {
                Some(p) => o.config = Some(p.into()),
                None => return Err("--config needs a file name".into()),
            },
            "-h" | "--help" => return Ok(Parsed::Help),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Parsed::Run(o))
}

/// Print to the console desk-pad was started from (or a message box).
fn tell(text: &str, error: bool) {
    if console::has_stdout() || console::attach_parent() {
        if error {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
    } else {
        win::message_box(text, error);
    }
}

fn main() -> ExitCode {
    let opts = match options() {
        Ok(Parsed::Run(o)) => o,
        Ok(Parsed::Help) => {
            tell(HELP, false);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            tell(&format!("{e}\n\n{HELP}"), true);
            return ExitCode::from(2);
        }
    };
    let Some(config) = opts.config.clone().or_else(config::default_path) else {
        tell("APPDATA is not set: use --config PATH", true);
        return ExitCode::FAILURE;
    };
    let tray = !opts.test && !opts.console;

    // A console of desk-pad's own when one is wanted and output goes nowhere
    // yet (output already redirected to a pipe or file is kept).
    let own_console = !tray && !console::has_stdout() && console::alloc();
    if !tray {
        console::install_ctrl_handler();
    }
    let log_file = tray.then(|| config.with_file_name("desk-pad.log"));
    if let Some(e) = logging::init(log_file.as_deref()) {
        logging::line("! ", &e);
    }
    std::panic::set_hook(Box::new(|info| logging::line("PANIC ", &info.to_string())));

    if win::already_running() {
        if tray {
            win::message_box(
                "desk-pad is already running: its icon is in the notification area \
                 (maybe behind the ^ arrow).",
                false,
            );
            return ExitCode::SUCCESS;
        }
        logging::notice(
            "Another desk-pad is running and acts on the pad too: \
             pause it from its tray icon.",
        );
    }

    let result = if tray {
        run_tray(opts, config, log_file)
    } else {
        app::run(&opts, &config, None)
    };
    stop::done();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            logging::line("! ", &e);
            if tray || own_console {
                win::message_box(&format!("desk-pad stopped: {e}"), true);
            }
            ExitCode::FAILURE
        }
    }
}

/// The input loop on its own thread, the tray icon on this one.
fn run_tray(opts: Options, config: PathBuf, log: Option<PathBuf>) -> Result<(), String> {
    let shared = Arc::new(Shared::default());
    let (commands, received) = mpsc::channel();
    {
        let shared = Arc::clone(&shared);
        logging::set_notify(move |msg| shared.notice(msg));
    }
    let autostart_args = opts
        .config
        .as_ref()
        .map(|p| format!("--config \"{}\"", p.display()))
        .unwrap_or_default();
    let link = Link {
        shared: Arc::clone(&shared),
        commands: received,
    };
    let input_config = config.clone();
    let input = thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(|| {
            app::run(&opts, &input_config, Some(&link))
        }))
        .unwrap_or_else(|_| Err("internal error (see the log)".into()));
        stop::done();
        link.shared.quit();
        result
    });
    let tray = win::tray::run(
        shared,
        commands,
        Paths {
            config,
            log,
            autostart_args,
        },
    );
    stop::request(); // the tray is gone: end the input loop too
    let input = input
        .join()
        .unwrap_or_else(|_| Err("internal error (see the log)".into()));
    tray.and(input)
}
