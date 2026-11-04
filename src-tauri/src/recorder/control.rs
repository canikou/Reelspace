//! State shared between the supervisor, the D-Bus service and clip jobs.
use super::config::Config;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    StateChanged {
        state: String,
        detail: String,
    },
    ConfigChanged,
    ClipSaving(u32),
    ClipSaved {
        path: String,
        duration: f64,
        game: String,
    },
    ClipFailed {
        reason: String,
        raw_path: String,
    },
    SessionStarted {
        game: String,
        path: String,
    },
    SessionEnded {
        game: String,
        path: String,
        duration: f64,
    },
    /// Needs a user-visible critical notification (already rate limited by the emitter).
    Critical {
        summary: String,
        body: String,
    },
}

pub trait Sink: Send + Sync {
    fn emit(&self, e: Event);
}

#[derive(Default)]
pub struct Flags {
    pub sounds: AtomicBool,
    pub notifications: AtomicBool,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub state: String,
    pub detail: String,
    pub replay_seconds: u32,
    pub archive_bytes: u64,
    pub free_bytes: u64,
    pub chunk_started: i64,
    pub session_game: String,
    pub last_error: String,
}

pub struct Control {
    cfg: Mutex<Config>,
    pub cfg_path: PathBuf,
    pub sock: PathBuf,
    pub status: Mutex<Status>,
    pub paused: AtomicBool,
    pub reload: AtomicBool,
    pub shutdown: AtomicBool,
    save_inflight: AtomicBool,
    /// Game currently detected (name, appid) — used for clip metadata.
    pub current_game: Mutex<Option<(String, Option<u32>)>>,
    pub focused_app_id: Mutex<Option<String>>,
    pub flags: Arc<Flags>,
    pub sink: Arc<dyn Sink>,
    last_critical: Mutex<Option<Instant>>,
}

impl Control {
    pub fn new(
        cfg: Config,
        cfg_path: PathBuf,
        sock: PathBuf,
        sink: Arc<dyn Sink>,
        flags: Arc<Flags>,
    ) -> Arc<Self> {
        flags.sounds.store(cfg.sounds, SeqCst);
        flags.notifications.store(cfg.notifications, SeqCst);
        Arc::new(Self {
            cfg: Mutex::new(cfg),
            cfg_path,
            sock,
            status: Mutex::new(Status {
                state: "stopped".into(),
                ..Default::default()
            }),
            paused: AtomicBool::new(false),
            reload: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            save_inflight: AtomicBool::new(false),
            current_game: Mutex::new(None),
            focused_app_id: Mutex::new(None),
            flags,
            sink,
            last_critical: Mutex::new(None),
        })
    }

    pub fn cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    pub fn set_cfg(&self, c: Config) {
        self.flags.sounds.store(c.sounds, SeqCst);
        self.flags.notifications.store(c.notifications, SeqCst);
        *self.cfg.lock().unwrap() = c;
    }

    pub fn set_state(&self, state: &str, detail: &str) {
        let mut s = self.status.lock().unwrap();
        let changed = s.state != state || s.detail != detail;
        let running = |st: &str| st == "recording" || st == "replay";
        // gsr just came up again: drop the stale failure, but keep clip errors raised while running.
        if running(state) && !running(&s.state) {
            s.last_error.clear();
        }
        s.state = state.into();
        s.detail = detail.into();
        if state == "error" {
            s.last_error = detail.into();
        }
        drop(s);
        if changed {
            self.sink.emit(Event::StateChanged {
                state: state.into(),
                detail: detail.into(),
            });
        }
    }

    /// Critical notification, at most one per `min_gap` (default callers use 10 min).
    pub fn critical(&self, summary: &str, body: &str, min_gap: Duration) {
        let mut last = self.last_critical.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < min_gap) {
            return;
        }
        *last = Some(Instant::now());
        self.sink.emit(Event::Critical {
            summary: summary.into(),
            body: body.into(),
        });
    }

    /// `true` if this caller won the right to save; a second press while one is in flight loses.
    pub fn begin_save(&self) -> bool {
        !self.save_inflight.swap(true, SeqCst)
    }

    pub fn end_save(&self) {
        self.save_inflight.store(false, SeqCst);
    }
}

/// Sink that records events (tests) or drops them.
#[derive(Default)]
pub struct VecSink(pub Mutex<Vec<Event>>);

impl Sink for VecSink {
    fn emit(&self, e: Event) {
        self.0.lock().unwrap().push(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_error_clears_when_recording_resumes() {
        let c = Control::new(
            Config::default(),
            "/x".into(),
            "/s".into(),
            Arc::new(VecSink::default()),
            Arc::default(),
        );
        c.set_state("error", "display gone");
        c.set_state("recording", "");
        assert_eq!(c.status.lock().unwrap().last_error, "");
        c.status.lock().unwrap().last_error = "clip failed".into();
        c.set_state("recording", "chunk 2");
        assert_eq!(c.status.lock().unwrap().last_error, "clip failed");
    }

    #[test]
    fn double_press_ignored_and_limiter() {
        let sink = Arc::new(VecSink::default());
        let c = Control::new(
            Config::default(),
            "/x".into(),
            "/s".into(),
            sink.clone(),
            Arc::default(),
        );
        assert!(c.begin_save());
        assert!(!c.begin_save());
        c.end_save();
        assert!(c.begin_save());
        c.critical("a", "b", Duration::from_secs(600));
        c.critical("a", "b", Duration::from_secs(600));
        assert_eq!(
            sink.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, Event::Critical { .. }))
                .count(),
            1
        );
        c.set_state("error", "x");
        c.set_state("error", "x");
        assert_eq!(
            sink.0
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, Event::StateChanged { .. }))
                .count(),
            1
        );
    }
}
