use reelspace_lib::db::Database;
use reelspace_lib::error::ErrorCode;
use reelspace_lib::ffmpeg::Ffmpeg;
use reelspace_lib::media::{scan_folder, FileFingerprint};
use std::collections::HashMap;
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn fixture(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("reelspace-fault-{name}-{nonce}"));
    fs::create_dir_all(&path).expect("fixture directory");
    path
}

fn tools() -> Ffmpeg {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("vendor")
        .join("ffmpeg")
        .join("bin");
    Ffmpeg::from_paths(directory.join("ffmpeg.exe"), directory.join("ffprobe.exe"))
        .expect("bundled tools")
}

#[test]
fn corrupt_media_isolated_without_aborting_the_source_scan() {
    let directory = fixture("corrupt-media");
    let source = directory.join("source");
    let thumbnails = directory.join("thumbnails");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&thumbnails).expect("thumbnails");
    let corrupt = source.join("corrupt.mp4");
    fs::write(&corrupt, b"not a media container").expect("corrupt media");
    File::options()
        .write(true)
        .open(&corrupt)
        .expect("fixture")
        .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(10)))
        .expect("age fixture");
    let database = Database::open(directory.join("library.sqlite")).expect("database");

    let result = scan_folder(
        &source,
        &tools(),
        &database,
        &thumbnails,
        &HashMap::new(),
        &[],
    )
    .expect("scan remains available");
    assert_eq!(result.files_skipped, 1);
    assert_eq!(result.items.len(), 0);
    assert_eq!(result.failures_by_code.get("ffprobeFailed"), Some(&1));
    drop(database);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn unavailable_sources_and_non_files_have_stable_error_codes() {
    let directory = fixture("invalid-inputs");
    let database = Database::open(directory.join("library.sqlite")).expect("database");
    let missing = directory.join("missing");
    let error = match scan_folder(
        &missing,
        &tools(),
        &database,
        &directory.join("thumbnails"),
        &HashMap::new(),
        &[],
    ) {
        Ok(_) => panic!("missing source must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::SourceUnavailable);
    let error = FileFingerprint::read(&directory).expect_err("directory is not media");
    assert_eq!(error.code, ErrorCode::InvalidPath);
    let empty = directory.join("empty.mp4");
    fs::write(&empty, []).expect("empty file");
    let error = FileFingerprint::read(&empty).expect_err("empty media");
    assert_eq!(error.code, ErrorCode::UnsupportedMedia);
    drop(database);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn tool_configuration_rejects_missing_executables_independently() {
    let missing = Path::new("Z:\\definitely-missing\\tool.exe").to_path_buf();
    let error = Ffmpeg::from_paths(missing.clone(), missing).expect_err("tools are absent");
    assert_eq!(error.code, ErrorCode::FfmpegUnavailable);
}
