use crate::error::{user_error, AppResult};
use crate::ffmpeg::Ffmpeg;
use crate::models::{CreateClipRequest, MediaItem, MediaKind};
use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "m4v"];

pub fn scan_folder(
    folder: &Path,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
    existing: &HashMap<String, MediaItem>,
    excluded_roots: &[PathBuf],
) -> AppResult<Vec<MediaItem>> {
    if !folder.exists() {
        return Err(user_error(format!(
            "source folder does not exist: {}",
            folder.display()
        )));
    }

    let files = WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path(), excluded_roots))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_video_file(path))
        .collect::<Vec<_>>();

    let items = files
        .par_iter()
        .filter_map(|path| {
            let key = normalized_path_key(path);
            if let Some(item) = existing.get(&key) {
                if file_is_unchanged(path, item) {
                    return Some(item.clone());
                }
            }
            build_raw_item(path, ffmpeg, thumbnail_dir).ok()
        })
        .collect();
    Ok(items)
}

pub fn build_clip_item(
    request: &CreateClipRequest,
    source: &MediaItem,
    output_path: PathBuf,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
) -> AppResult<MediaItem> {
    let stem = output_path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("clip");
    let temp_path = output_path.with_file_name(format!(".{stem}.reelspace-part.mp4"));
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
    if let Err(error) = trim_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    let usable = temp_path
        .metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if !usable {
        let _ = std::fs::remove_file(&temp_path);
        return Err(user_error("FFmpeg did not create a usable clip"));
    }
    if output_path.exists() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(user_error("a clip already exists at the generated path"));
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
    Ok(item)
}

pub fn ensure_thumbnail(
    id: &str,
    path: &Path,
    duration_seconds: f64,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
) -> AppResult<Option<PathBuf>> {
    let thumbnail_path = thumbnail_dir.join(format!("{id}.jpg"));
    let temp_path = thumbnail_dir.join(format!("{id}.part.jpg"));
    if is_usable_thumbnail(&thumbnail_path) {
        return Ok(Some(thumbnail_path));
    }

    let _ = std::fs::remove_file(&temp_path);

    let mut last_error = None;
    for at in thumbnail_offsets(duration_seconds) {
        match ffmpeg.thumbnail(path, &temp_path, at) {
            Ok(()) if is_usable_thumbnail(&temp_path) => {
                if thumbnail_path.exists() {
                    std::fs::remove_file(&thumbnail_path)?;
                }
                std::fs::rename(&temp_path, &thumbnail_path)?;
                return Ok(Some(thumbnail_path));
            }
            Ok(()) => {
                last_error = Some(user_error("FFmpeg produced an empty thumbnail"));
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

fn build_raw_item(path: &Path, ffmpeg: &Ffmpeg, thumbnail_dir: &Path) -> AppResult<MediaItem> {
    // This function is only reached for new or changed sources. A stable path
    // ID must not make a replacement file reuse the previous frame forever.
    let _ = std::fs::remove_file(thumbnail_dir.join(format!("{}.jpg", stable_id(path))));
    let mut item = build_item_base(path, MediaKind::Raw, ffmpeg, thumbnail_dir)?;
    item.tags = vec![infer_shadowplay_tag(path)];
    Ok(item)
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
    let probed = ffmpeg.probe(path).unwrap_or_default();
    let thumbnail =
        ensure_thumbnail(&id, path, probed.duration_seconds, ffmpeg, thumbnail_dir).unwrap_or(None);

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

pub fn normalized_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_excluded(path: &Path, excluded_roots: &[PathBuf]) -> bool {
    let key = normalized_path_key(path);
    excluded_roots.iter().any(|root| {
        let root_key = normalized_path_key(root);
        key == root_key || key.starts_with(&format!("{root_key}\\"))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_sanitizing_is_unicode_safe_and_nonempty() {
        let long = format!("{}é", "a".repeat(95));
        assert_eq!(sanitize_filename(&long).chars().count(), 96);
        assert_eq!(sanitize_filename("..."), "clip");
        assert_eq!(sanitize_filename("bad/name\0"), "bad_name_");
    }
}
