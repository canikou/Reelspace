use crate::commands::scan_sources_inner;
use crate::state::AppState;
use notify::{RecursiveMode, Watcher};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::PathBuf;
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
            let mut should_scan = last_reconcile.elapsed() >= Duration::from_secs(15 * 60);
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(_)) => {
                    std::thread::sleep(Duration::from_millis(1200));
                    while receiver.try_recv().is_ok() {}
                    should_scan = true;
                }
                Ok(Err(error)) => state.telemetry.event("warn", "library.watcher.event_failed", json!({ "error": error.to_string() })),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if should_scan && !watched.is_empty() {
                match scan_sources_inner(&state) {
                    Ok(_) => last_reconcile = Instant::now(),
                    Err(error) => state.telemetry.event("warn", "library.watcher.scan_failed", json!({ "code": error.code })),
                }
            }
        }
    }).ok();
}

fn stable_path_id(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))[..16].to_owned()
}
