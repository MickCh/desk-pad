//! The log: lines with the time since start, to the console (when there is
//! one) and to a file (when running in the tray). Notices, the messages the
//! user has to act on, are also handed to the tray icon to show.

use pad_core::input::Log;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

type Notify = Box<dyn Fn(&str) + Send>;

struct Sink {
    start: Instant,
    file: Option<File>,
    notify: Option<Notify>,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

fn sink() -> std::sync::MutexGuard<'static, Option<Sink>> {
    // A panic while logging must not stop all later logging.
    SINK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Start logging; with `file`, also to that file (truncated). Returns an
/// error message if the file can't be written (logging goes on without it).
pub fn init(file: Option<&Path>) -> Option<String> {
    let mut error = None;
    let file = file.and_then(|p| {
        let opened = p
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| File::create(p));
        opened
            .map_err(|e| error = Some(format!("cannot write the log {}: {e}", p.display())))
            .ok()
    });
    *sink() = Some(Sink {
        start: Instant::now(),
        file,
        notify: None,
    });
    error
}

/// Where notices go besides the log (the tray icon's notifications).
pub fn set_notify(f: impl Fn(&str) + Send + 'static) {
    if let Some(s) = sink().as_mut() {
        s.notify = Some(Box::new(f));
    }
}

/// Write errors are ignored: `println!` would panic on a closed pipe, and a
/// panic must not skip releasing held keys.
pub fn line(mark: &str, msg: &str) {
    let mut guard = sink();
    let text = match guard.as_ref() {
        Some(s) => format!("[{:8.3}] {mark}{msg}", s.start.elapsed().as_secs_f32()),
        None => format!("{mark}{msg}"),
    };
    let _ = writeln!(std::io::stdout(), "{text}");
    if let Some(f) = guard.as_mut().and_then(|s| s.file.as_mut()) {
        let _ = writeln!(f, "{text}");
    }
}

/// Something the user has to act on: logged and shown.
pub fn notice(msg: &str) {
    line("! ", msg);
    if let Some(f) = sink().as_ref().and_then(|s| s.notify.as_ref()) {
        f(msg);
    }
}

/// [`Log`] for the router and the rest of the input loop.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppLog;

impl Log for AppLog {
    fn log(&mut self, msg: &str) {
        line("", msg);
    }
    fn notice(&mut self, msg: &str) {
        notice(msg);
    }
}
