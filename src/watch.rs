//! Notices when the configuration file was saved: polls its modification
//! time and size, and reports a change once the file has stopped changing
//! (editors may write in several steps, or replace the file).

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// What identifies one version of the file; `None` = missing.
type Stamp = Option<(SystemTime, u64)>;

/// How often the file is checked.
const INTERVAL: Duration = Duration::from_millis(250);

pub struct Watcher {
    path: PathBuf,
    /// The version last reported (or found at start).
    seen: Stamp,
    /// A new version, and since when it has been unchanged.
    candidate: Option<(Stamp, Instant)>,
    next_check: Instant,
}

impl Watcher {
    pub fn new(path: PathBuf, now: Instant) -> Self {
        Watcher {
            seen: stamp(&path),
            path,
            candidate: None,
            next_check: now,
        }
    }

    /// Whether the file has changed since the last time this returned
    /// `true` (or since `new`). Cheap to call often.
    pub fn poll(&mut self, now: Instant) -> bool {
        if now < self.next_check {
            return false;
        }
        self.next_check = now + INTERVAL;
        let current = stamp(&self.path);
        self.update(current, now)
    }

    fn update(&mut self, current: Stamp, now: Instant) -> bool {
        if current == self.seen {
            self.candidate = None;
            return false;
        }
        match self.candidate {
            Some((c, since)) if c == current => {
                if now.duration_since(since) < INTERVAL {
                    return false;
                }
                self.seen = current;
                self.candidate = None;
                true
            }
            _ => {
                self.candidate = Some((current, now));
                false
            }
        }
    }
}

fn stamp(path: &PathBuf) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn version(secs: u64, len: u64) -> Stamp {
        Some((SystemTime::UNIX_EPOCH + Duration::from_secs(secs), len))
    }

    fn watcher(t0: Instant) -> Watcher {
        let mut w = Watcher::new(PathBuf::from("unused"), t0);
        w.seen = version(1, 100);
        w
    }

    #[test]
    fn reports_a_change_once_it_settled() {
        let t0 = Instant::now();
        let mut w = watcher(t0);
        assert!(!w.update(version(1, 100), t0), "unchanged");
        assert!(!w.update(version(2, 50), t0), "just changed");
        assert!(
            !w.update(version(2, 120), t0 + ms(250)),
            "still being written"
        );
        assert!(!w.update(version(2, 120), t0 + ms(300)), "not settled yet");
        assert!(w.update(version(2, 120), t0 + ms(500)));
        assert!(!w.update(version(2, 120), t0 + ms(750)), "reported once");
    }

    #[test]
    fn a_deleted_file_is_a_change_too() {
        let t0 = Instant::now();
        let mut w = watcher(t0);
        assert!(!w.update(None, t0));
        assert!(w.update(None, t0 + ms(250)));
        assert!(!w.update(None, t0 + ms(500)));
    }

    #[test]
    fn going_back_to_the_seen_version_is_no_change() {
        let t0 = Instant::now();
        let mut w = watcher(t0);
        assert!(!w.update(version(2, 1), t0));
        assert!(!w.update(version(1, 100), t0 + ms(250)));
        assert!(!w.update(version(1, 100), t0 + ms(500)));
    }

    #[test]
    fn polls_the_real_file() {
        let dir = std::env::temp_dir().join(format!("desk-pad-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("w.toml");
        std::fs::write(&path, "a").unwrap();
        let t0 = Instant::now();
        let mut w = Watcher::new(path.clone(), t0);
        assert!(!w.poll(t0));
        std::fs::write(&path, "longer").unwrap();
        assert!(!w.poll(t0 + ms(250)), "seen, not settled");
        assert!(w.poll(t0 + ms(500)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
