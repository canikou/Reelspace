use crate::error::AppResult;
use chrono::Utc;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOG_BACKUPS: usize = 3;

pub struct Telemetry {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl Telemetry {
    pub fn new(log_dir: &Path) -> AppResult<Self> {
        fs::create_dir_all(log_dir)?;
        Ok(Self {
            path: log_dir.join("reelspace.jsonl"),
            write_lock: Mutex::new(()),
        })
    }

    pub fn event(&self, level: &str, event: &str, fields: Value) {
        let record = json!({
            "timestamp": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "level": level,
            "event": event,
            "fields": fields,
        });
        let Ok(line) = serde_json::to_string(&record) else {
            return;
        };

        // Keep the runner log useful in development and persist the same
        // structured event for installed builds.
        eprintln!("{line}");
        let Ok(_guard) = self.write_lock.lock() else {
            return;
        };
        if let Err(error) = self.rotate_if_needed(line.len() as u64 + 1) {
            eprintln!(
                "{{\"timestamp\":\"{}\",\"level\":\"warn\",\"event\":\"telemetry.rotate.failed\",\"fields\":{{\"error\":{}}}}}",
                Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                serde_json::to_string(&error.to_string()).unwrap_or_else(|_| "\"unknown\"".into())
            );
        }
        let write_result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = write_result {
            let fallback_path = self.path.with_file_name("reelspace.fallback.jsonl");
            let fallback_result = OpenOptions::new()
                .create(true)
                .append(true)
                .open(fallback_path)
                .and_then(|mut file| writeln!(file, "{line}"));
            if fallback_result.is_err() {
                eprintln!("Reelspace could not persist telemetry: {error}");
            }
        }
    }

    fn rotate_if_needed(&self, incoming_bytes: u64) -> std::io::Result<()> {
        let current_bytes = self.path.metadata().map(|meta| meta.len()).unwrap_or(0);
        if current_bytes + incoming_bytes <= MAX_LOG_BYTES {
            return Ok(());
        }

        for index in (1..=LOG_BACKUPS).rev() {
            let source = rotated_path(&self.path, index);
            if index == LOG_BACKUPS && source.exists() {
                fs::remove_file(&source)?;
            } else if source.exists() {
                fs::rename(&source, rotated_path(&self.path, index + 1))?;
            }
        }
        if self.path.exists() {
            fs::rename(&self.path, rotated_path(&self.path, 1))?;
        }
        Ok(())
    }
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    path.with_file_name(format!("reelspace.{index}.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rotated_logs_have_stable_names() {
        assert_eq!(
            rotated_path(Path::new("C:\\logs\\reelspace.jsonl"), 2),
            PathBuf::from("C:\\logs\\reelspace.2.jsonl")
        );
    }

    #[test]
    fn events_are_written_as_json_lines() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("reelspace-telemetry-{nonce}"));
        let telemetry = Telemetry::new(&directory).expect("telemetry");
        telemetry.event("info", "test.event", json!({ "elapsedMs": 12 }));
        let contents = fs::read_to_string(directory.join("reelspace.jsonl")).expect("log file");
        let record: Value = serde_json::from_str(contents.trim()).expect("json record");
        assert_eq!(record["event"], "test.event");
        assert_eq!(record["fields"]["elapsedMs"], 12);
        drop(telemetry);
        let _ = fs::remove_dir_all(directory);
    }
}
