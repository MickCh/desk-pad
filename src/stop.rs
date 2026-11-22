//! Ending desk-pad cleanly: whoever wants it to stop (Ctrl+C, closing the
//! console, the tray menu, logging off) requests it; the input loop notices,
//! releases every key it holds and reports that it is done.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);

pub fn request() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// The input loop has released everything and ended.
pub fn done() {
    DONE.store(true, Ordering::SeqCst);
}

/// Wait up to `timeout` for [`done`]. For callers after whose return
/// Windows ends the process (console close, logoff).
pub fn wait_done(timeout: Duration) {
    let until = Instant::now() + timeout;
    while !DONE.load(Ordering::SeqCst) && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
}
