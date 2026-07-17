use crate::error::{user_error, AppResult};
use crate::ffmpeg::Ffmpeg;
use crate::models::{CreateClipRequest, MediaItem, MediaKind};
use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "m4v"];

pub fn scan_folder(
    folder: &Path,
    ffmpeg: &Ffmpeg,
    thumbnail_dir: &Path,
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
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_video_file(path))
        .collect::<Vec<_>>();

    let items = files
        .par_iter()
        .filter_map(|path| build_raw_item(path, ffmpeg, thumbnail_dir).ok())
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
    ffmpeg.trim(
        Path::new(&source.path),
        &output_path,
        request.start_seconds,
        request.end_seconds,
        request.accurate,
    )?;
    let mut item = build_item_base(&output_path, MediaKind::Clip, ffmpeg, thumbnail_dir)?;
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
    if is_usable_thumbnail(&thumbnail_path) {
        return Ok(Some(thumbnail_path));
    }

    if thumbnail_path.exists() {
        let _ = std::fs::remove_file(&thumbnail_path);
    }

    for at in thumbnail_offsets(duration_seconds) {
        if ffmpeg.thumbnail(path, &thumbnail_path, at).is_ok()
            && is_usable_thumbnail(&thumbnail_path)
        {
            return Ok(Some(thumbnail_path));
        }
        if thumbnail_path.exists() {
            let _ = std::fs::remove_file(&thumbnail_path);
        }
    }

    Ok(None)
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
    let start = request.start_seconds.round() as u64;
    let end = request.end_seconds.round() as u64;
    clip_dir.join(format!("{safe_title}_{start}-{end}.mp4"))
}

fn build_raw_item(path: &Path, ffmpeg: &Ffmpeg, thumbnail_dir: &Path) -> AppResult<MediaItem> {
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
    let mut out = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>();
    if out.len() > 96 {
        out.truncate(96);
    }
    out.trim_matches(['.', ' ']).to_string()
}
