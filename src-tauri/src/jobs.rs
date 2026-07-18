use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

pub struct MediaJobQueue {
    capacity: usize,
    active: Mutex<usize>,
    available: Condvar,
    sequence: AtomicU64,
}

pub struct MediaJobPermit {
    queue: Arc<MediaJobQueue>,
}

pub struct ThumbnailScheduler {
    running: AtomicBool,
    shutdown: AtomicBool,
    wake_generation: Mutex<u64>,
    wake_signal: Condvar,
}

impl Default for ThumbnailScheduler {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            wake_generation: Mutex::new(0),
            wake_signal: Condvar::new(),
        }
    }
}

impl ThumbnailScheduler {
    pub fn try_start(&self) -> bool {
        !self.shutdown.load(Ordering::Acquire)
            && self
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::Release);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.wake_signal.notify_all();
    }

    pub fn cancelled(&self) -> &AtomicBool {
        &self.shutdown
    }

    pub fn notify_work(&self) {
        let mut generation = self
            .wake_generation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *generation = generation.wrapping_add(1);
        self.wake_signal.notify_all();
    }

    pub fn wait_for_work(&self, timeout: Duration) -> bool {
        if self.shutdown.load(Ordering::Acquire) {
            return false;
        }
        let generation = self
            .wake_generation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let observed = *generation;
        let _guard = self
            .wake_signal
            .wait_timeout_while(generation, timeout, |current| {
                *current == observed && !self.shutdown.load(Ordering::Acquire)
            })
            .unwrap_or_else(|error| error.into_inner());
        !self.shutdown.load(Ordering::Acquire)
    }
}

impl MediaJobQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            active: Mutex::new(0),
            available: Condvar::new(),
            sequence: AtomicU64::new(1),
        }
    }

    pub fn next_id(&self, operation: &str) -> String {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        format!("{operation}-{sequence}")
    }

    pub fn active_count(&self) -> usize {
        *self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn acquire(
        self: &Arc<Self>,
        cancelled: Option<&AtomicBool>,
    ) -> Option<(MediaJobPermit, Duration, usize)> {
        let queued_at = Instant::now();
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while *active >= self.capacity {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                return None;
            }
            let (next, _) = self
                .available
                .wait_timeout(active, Duration::from_millis(100))
                .unwrap_or_else(|error| error.into_inner());
            active = next;
        }
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return None;
        }
        *active += 1;
        let active_count = *active;
        Some((
            MediaJobPermit {
                queue: Arc::clone(self),
            },
            queued_at.elapsed(),
            active_count,
        ))
    }
}

impl Drop for MediaJobPermit {
    fn drop(&mut self) {
        let mut active = self
            .queue
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *active = active.saturating_sub(1);
        self.queue.available.notify_one();
    }
}

#[derive(Default)]
pub struct PreviewCoordinator {
    output_locks: Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>,
    sessions: Mutex<HashMap<String, (Arc<AtomicBool>, Instant)>>,
}

impl PreviewCoordinator {
    pub fn output_lock(&self, output: &Path) -> Arc<Mutex<()>> {
        let mut locks = self
            .output_locks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(lock) = locks.get(output).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(output.to_path_buf(), Arc::downgrade(&lock));
        locks.retain(|_, lock| lock.strong_count() > 0);
        lock
    }

    pub fn session(&self, session_id: &str) -> Arc<AtomicBool> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        prune_sessions(&mut sessions);
        let (flag, touched_at) = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| (Arc::new(AtomicBool::new(false)), Instant::now()));
        *touched_at = Instant::now();
        Arc::clone(flag)
    }

    pub fn cancel_session(&self, session_id: &str) -> bool {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        prune_sessions(&mut sessions);
        let was_active = sessions.contains_key(session_id);
        let (flag, touched_at) = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| (Arc::new(AtomicBool::new(true)), Instant::now()));
        flag.store(true, Ordering::Release);
        *touched_at = Instant::now();
        was_active
    }
}

fn prune_sessions(sessions: &mut HashMap<String, (Arc<AtomicBool>, Instant)>) {
    const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
    sessions.retain(|_, (flag, touched_at)| {
        touched_at.elapsed() < SESSION_TTL || Arc::strong_count(flag) > 1
    });
}

pub fn lock_unpoisoned(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_are_monotonic() {
        let queue = MediaJobQueue::new(2);
        assert_eq!(queue.next_id("preview"), "preview-1");
        assert_eq!(queue.next_id("preview"), "preview-2");
    }

    #[test]
    fn cancelling_a_session_notifies_existing_jobs() {
        let coordinator = PreviewCoordinator::default();
        let flag = coordinator.session("trim-session");
        assert!(coordinator.cancel_session("trim-session"));
        assert!(flag.load(Ordering::Acquire));
        assert!(coordinator.session("trim-session").load(Ordering::Acquire));
    }

    #[test]
    fn cancelling_before_registration_keeps_late_jobs_cancelled() {
        let coordinator = PreviewCoordinator::default();
        assert!(!coordinator.cancel_session("late-session"));
        assert!(coordinator.session("late-session").load(Ordering::Acquire));
    }

    #[test]
    fn thumbnail_scheduler_allows_only_one_worker_and_stays_cancelled() {
        let scheduler = ThumbnailScheduler::default();
        assert!(scheduler.try_start());
        assert!(!scheduler.try_start());
        scheduler.finish();
        assert!(scheduler.try_start());
        scheduler.request_shutdown();
        scheduler.finish();
        assert!(!scheduler.try_start());
        assert!(scheduler.cancelled().load(Ordering::Acquire));
    }

    #[test]
    fn thumbnail_scheduler_wakes_early_for_new_work() {
        let scheduler = Arc::new(ThumbnailScheduler::default());
        let waiter = Arc::clone(&scheduler);
        let started = Instant::now();
        let thread = std::thread::spawn(move || waiter.wait_for_work(Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(20));
        scheduler.notify_work();
        assert!(thread.join().expect("waiter"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
