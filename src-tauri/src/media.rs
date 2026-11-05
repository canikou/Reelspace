use crate::db::Database;
use crate::error::{app_error, user_error, AppResult, ErrorCode};
use crate::ffmpeg::{Ffmpeg, TrimMethod};
use crate::models::{CreateClipRequest, MediaItem, MediaKind};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "m4v"];

pub struct ScanFolderResult {
    pub items: Vec<MediaItem>,
    pub files_visited: usize,
    pub files_changed: usize,
    pub cached_probe_hits: usize,
    pub files_reprobed: usize,
    pub files_skipped: usize,
    pub traversal_errors: usize,
    pub changed_path_keys: HashSet<String>,
    pub failures_by_code: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub path_key: String,
    pub size: u64,
    pub modified_ns: u128,
}

impl FileFingerprint {
    pub fn read(path: &Path) -> AppResult<Self> {
        let metadata = path.metadata()?;
        if !metadata.is_file() {
            return Err(app_error(
                ErrorCode::InvalidPath,
                "The media path is not a file",
            ));
        }
        if metadata.len() == 0 {
            return Err(app_error(
                ErrorCode::UnsupportedMedia,
                "Empty media files are not indexed",
            ));
        }
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        Ok(Self {
            path_key: normalized_path_key(
                &path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
            ),
            size: metadata.len(),
            modified_ns,
        })
    }

    pub fn token(&self) -> String {
        format!("{}:{}:{}", self.path_key, self.size, self.modified_ns)
    }

    pub fn read_stable(path: &Path, minimum_age: Duration) -> AppResult<Self> {
        let fingerprint = Self::read(path)?;
        let metadata = path.metadata()?;
        let is_recent = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age < minimum_age);
        if is_recent {
            return Err(app_error(
                ErrorCode::FileChanged,
                "A recording is still being written and will be retried later",
            ));
        }
        Ok(fingerprint)
    }
}

pub fn scan_folder(
    folder: &Path,
    ffmpeg: &Ffmpeg,
    database: &Database,
    thumbnail_dir: &Path,
    existing: &HashMap<String, MediaItem>,
    excluded_roots: &[PathBuf],
) -> AppResult<ScanFolderResult> {
    if !folder.is_dir() {
        return Err(app_error(
            ErrorCode::SourceUnavailable,
            "A configured source folder is unavailable",
        )
        .with_detail(folder.display().to_string()));
    }

    let clip_dir = recorder_clip_dir();
    let mut files = Vec::new();
    let mut files_visited = 0;
    let mut traversal_errors = 0;
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path(), excluded_roots))
    {
        match entry {
            Ok(entry) => {
                files_visited += 1;
                if entry.file_type().is_file()
                    && is_video_file(entry.path())
                    && !is_in_progress_recording(entry.path())
                {
                    files.push(entry.into_path());
                }
            }
            Err(_) => traversal_errors += 1,
        }
    }

    let mut result = ScanFolderResult {
        items: Vec::with_capacity(files.len()),
        files_visited,
        files_changed: 0,
        cached_probe_hits: 0,
        files_reprobed: 0,
        files_skipped: 0,
        traversal_errors,
        changed_path_keys: HashSet::new(),
        failures_by_code: HashMap::new(),
    };
    for path in files {
        let key = normalized_path_key(&path);
        if let Some(item) = existing.get(&key) {
            if file_is_unchanged(&path, item) {
                if let Ok(fingerprint) = FileFingerprint::read_stable(&path, Duration::from_secs(3))
                {
                    if database.get_probe_cache(&fingerprint)?.is_some() {
                        result.cached_probe_hits += 1;
                        result.items.push(item.clone());
                        continue;
                    }
                }
            }
        }
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_raw_item(&path, ffmpeg, database, thumbnail_dir, clip_dir.as_deref())
        }));
        match built {
            Ok(Ok((item, cache_hit))) => {
                result.files_changed += 1;
                result.changed_path_keys.insert(key);
                if cache_hit {
                    result.cached_probe_hits += 1;
                } else {
                    result.files_reprobed += 1;
                }
                result.items.push(item);
            }
            Ok(Err(error)) => {
                result.files_skipped += 1;
                *result
                    .failures_by_code
                    .entry(error.code.as_str().to_string())
                    .or_default() += 1;
            }
            Err(_) => {
                result.files_skipped += 1;
                *result
                    .failures_by_code
                    .entry(ErrorCode::Internal.as_str().to_string())
                    .or_default() += 1;
            }
        }
    }
    Ok(result)
}

pub fn build_clip_item(
    request: &CreateClipRequest,
    source: &MediaItem,
    output_path: PathBuf,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
) -> AppResult<(MediaItem, TrimMethod)> {
    let stem = output_path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("clip");
    let source_fingerprint = FileFingerprint::read(Path::new(&source.path))?;
    let temp_path = unique_temp_path(&output_path, stem, "mp4");
    if temp_path.exists() {
        let _ = std::fs::remove_file(&temp_path);
    }
    let trim_result = ffmpeg.trim(
        Path::new(&source.path),
        &temp_path,
        request.start_seconds,
        request.end_seconds,
        true,
        request
            .enabled_audio_track_indices
            .as_deref()
            .unwrap_or_default(),
    );
    let trim_method = match trim_result {
        Ok(method) => method,
        Err(error) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    let usable = temp_path
        .metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if !usable {
        let _ = std::fs::remove_file(&temp_path);
        return Err(app_error(
            ErrorCode::ExportFailed,
            "FFmpeg did not create a usable clip",
        ));
    }
    if FileFingerprint::read(Path::new(&source.path))? != source_fingerprint {
        let _ = std::fs::remove_file(&temp_path);
        return Err(app_error(
            ErrorCode::FileChanged,
            "The source recording changed during export; no clip was saved",
        ));
    }
    if output_path.exists() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(app_error(
            ErrorCode::DestinationCollision,
            "A clip already exists at the generated destination",
        ));
    }
    if let Err(error) = std::fs::rename(&temp_path, &output_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }
    let mut item = match build_item_base(&output_path, MediaKind::Clip, ffmpeg, thumbnail_dir) {
        Ok(item) => item,
        Err(error) => {
            let _ = std::fs::remove_file(&output_path);
            return Err(error);
        }
    };
    item.source_path = Some(source.path.clone());
    item.tags = source.tags.clone();
    item.notes = request.title.clone();
    item.trim_start_seconds = Some(request.start_seconds);
    item.trim_end_seconds = Some(request.end_seconds);
    Ok((item, trim_method))
}

pub fn ensure_thumbnail(
    id: &str,
    path: &Path,
    duration_seconds: f64,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> AppResult<Option<PathBuf>> {
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(user_error("thumbnail media identifier is invalid"));
    }
    let thumbnail_path = thumbnail_dir.join(format!("{id}.jpg"));
    let temp_path = unique_temp_path(&thumbnail_path, id, "jpg");
    if is_usable_thumbnail(&thumbnail_path) {
        return Ok(Some(thumbnail_path));
    }
    let source_fingerprint = FileFingerprint::read(path)?;

    let _ = std::fs::remove_file(&temp_path);

    let mut last_error = None;
    for at in thumbnail_offsets(duration_seconds) {
        match ffmpeg.thumbnail_cancellable(path, &temp_path, at, cancelled) {
            Ok(()) if is_usable_thumbnail(&temp_path) => {
                let unchanged = FileFingerprint::read(path)
                    .map(|current| current == source_fingerprint)
                    .unwrap_or(false);
                if !unchanged {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(app_error(
                        ErrorCode::FileChanged,
                        "The source recording changed during thumbnail generation",
                    ));
                }
                if thumbnail_path.exists() {
                    std::fs::remove_file(&thumbnail_path)?;
                }
                std::fs::rename(&temp_path, &thumbnail_path)?;
                return Ok(Some(thumbnail_path));
            }
            Ok(()) => {
                last_error = Some(app_error(
                    ErrorCode::ThumbnailGenerationFailed,
                    "FFmpeg produced an invalid thumbnail",
                ));
            }
            Err(error) if error.code == ErrorCode::MediaProcessCancelled => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(error);
            }
            Err(error) => last_error = Some(error),
        }
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
        }
    }

    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(None)
    }
}

pub fn clip_output_path(
    clip_dir: &Path,
    request: &CreateClipRequest,
    source: &MediaItem,
) -> PathBuf {
    let safe_title = request
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(sanitize_filename)
        .unwrap_or_else(|| sanitize_filename(&source.filename));
    let start = (request.start_seconds * 1000.0).round().max(0.0) as u64;
    let end = (request.end_seconds * 1000.0).round().max(0.0) as u64;
    let base = clip_dir.join(format!("{safe_title}_{start}-{end}.mp4"));
    unique_path(base)
}

fn build_raw_item(
    path: &Path,
    ffmpeg: &Ffmpeg,
    database: &Database,
    thumbnail_dir: &Path,
    recorder_clips: Option<&Path>,
) -> AppResult<(MediaItem, bool)> {
    // This function is only reached for new or changed sources. A stable path
    // ID must not make a replacement file reuse the previous frame forever.
    let _ = std::fs::remove_file(thumbnail_dir.join(format!("{}.jpg", stable_id(path))));
    let fingerprint = FileFingerprint::read_stable(path, Duration::from_secs(3))?;
    let (probed, cache_hit) = match database.get_probe_cache(&fingerprint)? {
        Some(probed) => (probed, true),
        None => {
            let probed = ffmpeg.probe_data(path)?;
            database.put_probe_cache(&fingerprint, &probed)?;
            (probed, false)
        }
    };
    let metadata = std::fs::metadata(path)?;
    let modified_at = timestamp_seconds(metadata.modified().ok());
    let created_at = metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(modified_at);
    // Recorder files carry their real start time in the name; mtime is when the chunk ended.
    let created_at = recording_timestamp(path).unwrap_or(created_at);
    let id = stable_id(path);
    let mut item = MediaItem {
        id,
        kind: if is_within(path, recorder_clips) {
            MediaKind::Clip
        } else {
            MediaKind::Raw
        },
        path: path.to_string_lossy().to_string(),
        source_path: None,
        filename: path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("video")
            .to_string(),
        file_size: metadata.len(),
        duration_seconds: probed.metadata.duration_seconds,
        width: probed.metadata.width,
        height: probed.metadata.height,
        codec: probed.metadata.codec,
        tags: Vec::new(),
        notes: None,
        thumbnail_path: None,
        created_at,
        modified_at,
        trim_start_seconds: None,
        trim_end_seconds: None,
    };
    item.tags = vec![infer_shadowplay_tag(path)];
    Ok((item, cache_hit))
}

/// Recorder's still-growing `Video_<ts>.mkv`; it gets renamed when finalised.
fn is_in_progress_recording(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        path.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|n| crate::recorder::files::video_timestamp(n).is_some())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        false
    }
}

fn is_within(path: &Path, root: Option<&Path>) -> bool {
    root.is_some_and(|root| {
        path_key_is_within(&normalized_path_key(path), &normalized_path_key(root))
    })
}

/// True for files the recorder daemon saved as clips (Linux only).
pub fn is_recorder_clip(path: &Path) -> bool {
    is_within(path, recorder_clip_dir().as_deref())
}

/// The recorder's clip folder; None off Linux or when its config is invalid.
#[cfg(target_os = "linux")]
pub fn recorder_clip_dir() -> Option<PathBuf> {
    recorder_clip_dir_from(&crate::recorder::config::config_path())
}

#[cfg(not(target_os = "linux"))]
pub fn recorder_clip_dir() -> Option<PathBuf> {
    None
}

/// The recorder's recordings folder when the recorder is installed and the folder exists (Linux only).
#[cfg(target_os = "linux")]
pub fn recorder_rec_dir() -> Option<PathBuf> {
    if !crate::features::recorder_supported() {
        return None;
    }
    let dir = crate::recorder::config::Config::load(&crate::recorder::config::config_path())
        .ok()?
        .rec_dir;
    dir.is_dir().then_some(dir)
}

#[cfg(not(target_os = "linux"))]
pub fn recorder_rec_dir() -> Option<PathBuf> {
    None
}

/// Configured sources plus the recorder folder, unless it or a parent is already a source.
/// A child source (e.g. rec_dir/Clips) stays; the caller dedupes items by path.
pub fn effective_sources(configured: &[String]) -> Vec<String> {
    with_recorder_dir(configured, recorder_rec_dir())
}

fn with_recorder_dir(configured: &[String], rec_dir: Option<PathBuf>) -> Vec<String> {
    let mut out = configured.to_vec();
    if let Some(dir) = rec_dir {
        let key = normalized_path_key(&dir);
        if !configured
            .iter()
            .any(|s| path_key_is_within(&key, &normalized_path_key(Path::new(s))))
        {
            out.push(dir.to_string_lossy().to_string());
        }
    }
    out
}

/// Local-time `YYYY-MM-DD_HH-MM-SS` in a rec_/session_/clip_ file name, as unix seconds.
pub fn recording_timestamp(path: &Path) -> Option<i64> {
    let name = path.file_stem()?.to_str()?;
    if !(name.starts_with("rec_") || name.starts_with("session_") || name.starts_with("clip_")) {
        return None;
    }
    let ts = name.get(name.len().checked_sub(19)?..)?;
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d_%H-%M-%S")
        .ok()?
        .and_local_timezone(chrono::Local)
        .earliest()
        .map(|t| t.timestamp())
}

#[cfg(target_os = "linux")]
fn recorder_clip_dir_from(config: &Path) -> Option<PathBuf> {
    crate::recorder::config::Config::load(config)
        .ok()
        .map(|cfg| cfg.clip_dir())
}

fn build_item_base(
    path: &Path,
    kind: MediaKind,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
) -> AppResult<MediaItem> {
    let metadata = std::fs::metadata(path)?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_else(|| Utc::now().timestamp());
    let created_at = metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(modified_at);
    let id = stable_id(path);
    let probed = ffmpeg.probe(path)?;
    let thumbnail = ensure_thumbnail(
        &id,
        path,
        probed.duration_seconds,
        ffmpeg,
        thumbnail_dir,
        None,
    )
    .unwrap_or(None);

    Ok(MediaItem {
        id,
        kind,
        path: path.to_string_lossy().to_string(),
        source_path: None,
        filename: path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("video")
            .to_string(),
        file_size: metadata.len(),
        duration_seconds: probed.duration_seconds,
        width: probed.width,
        height: probed.height,
        codec: probed.codec,
        tags: Vec::new(),
        notes: None,
        thumbnail_path: thumbnail.map(|path| path.to_string_lossy().to_string()),
        created_at,
        modified_at,
        trim_start_seconds: None,
        trim_end_seconds: None,
    })
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

/// Separator used inside keys produced by [`normalized_path_key`].
pub const PATH_KEY_SEPARATOR: char = if cfg!(windows) { '\\' } else { '/' };

/// Identity key for a path. Windows filesystems are case-insensitive, so keys
/// fold case and separators there; Unix keys keep case because `a.mp4` and
/// `A.mp4` are different files.
#[cfg(windows)]
pub fn normalized_path_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('/', "\\").to_lowercase();
    if let Some(rest) = key.strip_prefix("\\\\?\\unc\\") {
        key = format!("\\\\{rest}");
    } else if let Some(rest) = key.strip_prefix("\\\\?\\") {
        key = rest.to_string();
    }
    key.trim_end_matches('\\').to_string()
}

#[cfg(not(windows))]
pub fn normalized_path_key(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_string()
}

/// True when `key` equals `root_key` or lies beneath it (both from [`normalized_path_key`]).
pub fn path_key_is_within(key: &str, root_key: &str) -> bool {
    key == root_key
        || key
            .strip_prefix(root_key)
            .is_some_and(|rest| rest.starts_with(PATH_KEY_SEPARATOR))
}

fn is_excluded(path: &Path, excluded_roots: &[PathBuf]) -> bool {
    let key = normalized_path_key(path);
    excluded_roots
        .iter()
        .any(|root| path_key_is_within(&key, &normalized_path_key(root)))
}

fn file_is_unchanged(path: &Path, item: &MediaItem) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    metadata.len() == item.file_size && modified_at == item.modified_at
}

fn timestamp_seconds(time: Option<std::time::SystemTime>) -> i64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_else(|| Utc::now().timestamp())
}

fn unique_path(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base.file_stem().and_then(OsStr::to_str).unwrap_or("clip");
    for suffix in 2..=10_000 {
        let candidate = parent.join(format!("{stem}-{suffix}.mp4"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-{}.mp4", Utc::now().timestamp_millis()))
}

fn infer_shadowplay_tag(path: &Path) -> String {
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .unwrap_or("Desktop")
        .trim();
    if parent.is_empty() || parent.eq_ignore_ascii_case("desktop") {
        "Desktop".to_string()
    } else {
        parent.to_string()
    }
}

fn stable_id(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn is_usable_thumbnail(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 1024)
        .unwrap_or(false)
}

fn thumbnail_offsets(duration_seconds: f64) -> Vec<f64> {
    let mut offsets: Vec<f64> = Vec::new();
    let candidates = if duration_seconds > 20.0 {
        vec![
            5.0,
            duration_seconds * 0.25,
            duration_seconds * 0.5,
            1.0,
            0.0,
        ]
    } else if duration_seconds > 3.0 {
        vec![1.0, duration_seconds * 0.5, 0.0]
    } else {
        vec![0.0, 0.5]
    };

    for value in candidates {
        let clamped = if duration_seconds > 0.0 {
            value.min((duration_seconds - 0.1).max(0.0))
        } else {
            value.max(0.0)
        };
        if !offsets
            .iter()
            .any(|existing| (existing - clamped).abs() < 0.05)
        {
            offsets.push(clamped);
        }
    }

    offsets
}

fn sanitize_filename(value: &str) -> String {
    let out = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            _ => ch,
        })
        .take(96)
        .collect::<String>();
    let cleaned = out.trim_matches(['.', ' ']);
    if cleaned.is_empty() {
        "clip".to_string()
    } else {
        cleaned.to_string()
    }
}

fn unique_temp_path(output_path: &Path, stem: &str, extension: &str) -> PathBuf {
    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    output_path.with_file_name(format!(
        ".{stem}-{}-{sequence}.reelspace-part.{extension}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_timestamp_from_names() {
        use chrono::TimeZone;
        let want = chrono::Local
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap()
            .timestamp();
        for n in [
            "rec_2026-01-02_03-04-05.mkv",
            "session_Elden_Ring_2026-01-02_03-04-05.mkv",
            "clip_2026-01-02_03-04-05.mp4",
        ] {
            assert_eq!(
                recording_timestamp(Path::new(&format!("/x/{n}"))),
                Some(want),
                "{n}"
            );
        }
        assert_eq!(
            recording_timestamp(Path::new("/x/holiday_2026-01-02_03-04-05.mp4")),
            None
        );
        assert_eq!(recording_timestamp(Path::new("/x/rec_nope.mkv")), None);
    }

    #[test]
    fn recorder_dir_added_unless_covered() {
        let rec = || Some(PathBuf::from("/v/Rec"));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(with_recorder_dir(&s(&["/a"]), rec()), s(&["/a", "/v/Rec"]));
        assert_eq!(with_recorder_dir(&s(&["/v"]), rec()), s(&["/v"]));
        assert_eq!(with_recorder_dir(&s(&["/v/Rec"]), rec()), s(&["/v/Rec"]));
        assert_eq!(
            with_recorder_dir(&s(&["/v/Rec/Clips"]), rec()),
            s(&["/v/Rec/Clips", "/v/Rec"])
        );
        assert_eq!(with_recorder_dir(&s(&["/a"]), None), s(&["/a"]));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recorder_clip_dir_follows_config_and_tolerates_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("recorder.json");
        // Missing file falls back to defaults; invalid JSON is skipped.
        assert!(recorder_clip_dir_from(&cfg).is_some());
        std::fs::write(&cfg, "{nope").unwrap();
        assert_eq!(recorder_clip_dir_from(&cfg), None);
        std::fs::write(&cfg, r#"{"rec_dir":"/data/rec"}"#).unwrap();
        let clips = recorder_clip_dir_from(&cfg).unwrap();
        assert_eq!(clips, PathBuf::from("/data/rec/Clips"));
        assert!(is_within(
            Path::new("/data/rec/Clips/clip_1.mp4"),
            Some(&clips)
        ));
        assert!(!is_within(
            Path::new("/data/rec/Clips2/a.mp4"),
            Some(&clips)
        ));
        assert!(!is_within(Path::new("/data/rec/raw.mp4"), Some(&clips)));
    }

    #[test]
    fn filename_sanitizing_is_unicode_safe_and_nonempty() {
        let long = format!("{}é", "a".repeat(95));
        assert_eq!(sanitize_filename(&long).chars().count(), 96);
        assert_eq!(sanitize_filename("..."), "clip");
        assert_eq!(sanitize_filename("bad/name\0"), "bad_name_");
    }

    #[cfg(windows)]
    #[test]
    fn normalized_windows_paths_collapse_verbatim_drive_and_unc_prefixes() {
        assert_eq!(
            normalized_path_key(Path::new(r"\\?\D:\Captures\Clip.mp4")),
            normalized_path_key(Path::new(r"D:\Captures\Clip.mp4"))
        );
        assert_eq!(
            normalized_path_key(Path::new(r"\\?\UNC\server\share\Clip.mp4")),
            normalized_path_key(Path::new(r"\\server\share\Clip.mp4"))
        );
    }

    #[test]
    fn temporary_outputs_stay_next_to_the_destination() {
        let output = Path::new("C:\\clips\\final.mp4");
        let temporary = unique_temp_path(output, "final", "mp4");
        assert_eq!(temporary.parent(), output.parent());
        assert!(temporary
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".reelspace-part.mp4"));
    }

    #[test]
    fn recently_modified_recordings_are_deferred() {
        let path = std::env::temp_dir().join(format!(
            "reelspace-growing-{}-{}.mp4",
            std::process::id(),
            TEMP_SEQUENCE_FOR_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, [1_u8, 2, 3]).expect("fixture");
        let error = FileFingerprint::read_stable(&path, Duration::from_secs(3))
            .expect_err("fresh file must be deferred");
        assert_eq!(error.code, ErrorCode::FileChanged);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn clip_destinations_never_overwrite_an_existing_file() {
        let directory = std::env::temp_dir().join(format!(
            "reelspace-clip-collision-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE_FOR_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("directory");
        let source = MediaItem {
            id: "a".repeat(64),
            kind: MediaKind::Raw,
            path: directory.join("source.mp4").to_string_lossy().to_string(),
            source_path: None,
            filename: "source.mp4".to_string(),
            file_size: 1,
            duration_seconds: 10.0,
            width: None,
            height: None,
            codec: None,
            tags: Vec::new(),
            notes: None,
            thumbnail_path: None,
            created_at: 1,
            modified_at: 1,
            trim_start_seconds: None,
            trim_end_seconds: None,
        };
        let request = CreateClipRequest {
            source_id: source.id.clone(),
            session_id: None,
            start_seconds: 1.0,
            end_seconds: 2.0,
            title: Some("highlight".to_string()),
            enabled_audio_track_indices: Some(Vec::new()),
        };
        let first = clip_output_path(&directory, &request, &source);
        std::fs::write(&first, b"original").expect("existing clip");
        let second = clip_output_path(&directory, &request, &source);
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).expect("preserved"), b"original");
        let _ = std::fs::remove_dir_all(directory);
    }

    static TEMP_SEQUENCE_FOR_TEST: AtomicU64 = AtomicU64::new(1);
}
