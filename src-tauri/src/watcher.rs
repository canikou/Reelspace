use crate::commands::scan_sources_inner;
use crate::state::AppState;
use notify::{RecursiveMode, Watcher};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn start_library_watcher(state: AppState) {
    std::thread::Builder::new().name("reelspace-library-watcher".into()).spawn(move || {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |event| { let _ = sender.send(event); }) {
            Ok(watcher) => watcher,
            Err(error) => {
                state.telemetry.event("error", "library.watcher.unavailable", json!({ "error": error.to_string() }));
                return;
            }
        };
        let mut watched = BTreeSet::<PathBuf>::new();
        let mut last_reconcile = Instant::now();
        let mut last_scan_end: Option<Instant> = None;
        let mut pending = false;
        loop {
            if let Ok(settings) = state.db.load_settings() {
                let wanted = settings.source_folders.into_iter().map(PathBuf::from).filter(|path| path.is_dir()).collect::<BTreeSet<_>>();
                for path in watched.difference(&wanted) { let _ = watcher.unwatch(path); }
                for path in wanted.difference(&watched) {
                    if let Err(error) = watcher.watch(path, RecursiveMode::Recursive) {
                        state.telemetry.event("warn", "library.watcher.path_failed", json!({ "pathId": stable_path_id(path), "error": error.to_string() }));
                    }
                }
                watched = wanted;
            }
            if last_reconcile.elapsed() >= Duration::from_secs(15 * 60) { pending = true; }
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(_)) => {
                    std::thread::sleep(Duration::from_millis(1200));
                    while receiver.try_recv().is_ok() {}
                    pending = true;
                }
                Ok(Err(error)) => state.telemetry.event("warn", "library.watcher.event_failed", json!({ "error": error.to_string() })),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            // At most one watcher scan per minute, and never while another scan runs; events in between coalesce into `pending`.
            let gap_ok = last_scan_end.is_none_or(|end| end.elapsed() >= Duration::from_secs(60));
            if pending && gap_ok && !watched.is_empty() && state.scan_in_flight.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                pending = false;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan_sources_inner(&state)));
                state.scan_in_flight.store(false, Ordering::Release);
                last_scan_end = Some(Instant::now());
                match outcome {
                    Ok(Ok(_)) => last_reconcile = Instant::now(),
                    Ok(Err(error)) => state.telemetry.event("warn", "library.watcher.scan_failed", json!({ "code": error.code })),
                    Err(_) => state.telemetry.event("error", "library.watcher.scan_panicked", json!({})),
                }
            }
        }
    }).ok();
}

fn stable_path_id(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..16].to_owned()
}
