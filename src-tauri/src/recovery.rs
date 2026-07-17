use crate::error::AppResult;
use chrono::Utc;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};

static CRASH_DIRECTORY: OnceLock<Mutex<PathBuf>> = OnceLock::new();
static INSTALL_HOOK: Once = Once::new();

pub struct RecoveryJournal {
    marker_path: PathBuf,
    pub recovered_unclean_session: bool,
}

impl RecoveryJournal {
    pub fn begin(data_dir: &Path) -> AppResult<Self> {
        let recovery_dir = data_dir.join("recovery");
        fs::create_dir_all(&recovery_dir)?;
        install_crash_handler(recovery_dir.clone());
        let marker_path = recovery_dir.join("active-session.json");
        let recovered_unclean_session = marker_path.is_file();
        if recovered_unclean_session {
            let archived = recovery_dir.join(format!(
                "unclean-session-{}-{}.json",
                Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
                std::process::id()
            ));
            let _ = fs::copy(&marker_path, archived);
        }
        let marker = serde_json::to_vec_pretty(&json!({
            "pid": std::process::id(),
            "startedAt": Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION"),
        }))?;
        atomic_write(&marker_path, &marker)?;
        Ok(Self {
            marker_path,
            recovered_unclean_session,
        })
    }

    pub fn finish(&self) {
        let _ = fs::remove_file(&self.marker_path);
    }
}

pub fn record_startup_failure(error: &str) {
    write_report("startup-failure", json!({ "error": error }));
}

fn install_crash_handler(directory: PathBuf) {
    let crash_directory = CRASH_DIRECTORY.get_or_init(|| Mutex::new(directory.clone()));
    *crash_directory
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = directory;
    INSTALL_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            let payload = panic
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            let location = panic
                .location()
                .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column()));
            let thread = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            write_report(
                "panic",
                json!({ "thread": thread, "location": location, "message": payload }),
            );
            previous(panic);
        }));
    });
}

fn write_report(kind: &str, fields: serde_json::Value) {
    let Some(directory) = CRASH_DIRECTORY.get() else {
        return;
    };
    let directory = directory
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let _ = fs::create_dir_all(&directory);
    let path = directory.join(format!(
        "{kind}-{}-{}.json",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    ));
    let report = json!({
        "kind": kind,
        "timestamp": Utc::now().to_rfc3339(),
        "pid": std::process::id(),
        "fields": fields,
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        let _ = atomic_write(&path, &bytes);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("part-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stale_session_markers_are_detected_and_replaced() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("reelspace-recovery-{nonce}"));
        let first = RecoveryJournal::begin(&directory).expect("first session");
        assert!(!first.recovered_unclean_session);
        let second = RecoveryJournal::begin(&directory).expect("second session");
        assert!(second.recovered_unclean_session);
        second.finish();
        assert!(!directory.join("recovery/active-session.json").exists());
        let _ = fs::remove_dir_all(directory);
    }
}
