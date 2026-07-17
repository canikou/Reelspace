use crate::error::{user_error, AppResult};
use crate::explorer;
use crate::media;
use crate::models::{
    AppSettings, AppSnapshot, AudioTrack, Collection, CreateClipRequest, MediaItem, MediaKind,
    StorageStats,
};
use crate::state::AppState;
use crate::storage;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::State;

#[tauri::command]
pub fn get_app_state(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    let settings = state.db.load_settings()?;
    let media = state.db.list_media(None)?;
    let stats = storage::stats(&media, &settings);
    Ok(AppSnapshot {
        settings,
        ffmpeg: state.ffmpeg.health(),
        stats,
    })
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> AppResult<AppSnapshot> {
    validate_settings(&settings)?;
    state.db.save_settings(&settings)?;
    get_app_state(state)
}

#[tauri::command]
pub fn scan_sources(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let _index_guard = state
        .media_index_lock
        .lock()
        .map_err(|_| user_error("media index lock was poisoned"))?;
    eprintln!("[media] scanning configured sources");
    let settings = state.db.load_settings()?;
    let existing_items = state.db.list_media(Some(MediaKind::Raw))?;
    let existing = existing_items
        .iter()
        .cloned()
        .map(|item| (media::normalized_path_key(Path::new(&item.path)), item))
        .collect::<HashMap<_, _>>();
    let excluded_roots = output_roots(&settings);
    let mut scanned = Vec::new();
    for folder in &settings.source_folders {
        scanned.extend(media::scan_folder(
            Path::new(folder),
            &state.ffmpeg,
            &state.thumbnail_dir,
            &existing,
            &excluded_roots,
        )?);
    }
    eprintln!("[media] indexed {} source files", scanned.len());
    for item in &scanned {
        let key = media::normalized_path_key(Path::new(&item.path));
        let unchanged = existing
            .get(&key)
            .map(|current| {
                current.file_size == item.file_size && current.modified_at == item.modified_at
            })
            .unwrap_or(false);
        if !unchanged {
            state.db.upsert_media(item)?;
        }
    }
    for item in existing_items {
        let path = Path::new(&item.path);
        if belongs_to_sources(path, &settings.source_folders)
            && !belongs_to_roots(path, &excluded_roots)
            && !path.exists()
        {
            state.db.delete_media(&item.id)?;
        }
    }
    // Backfill thumbnails for unchanged records too. Older versions could
    // import media successfully while silently leaving thumbnail_path empty.
    let (repaired, failures) = repair_missing_thumbnails(&state)?;
    eprintln!("[media] repaired {repaired} thumbnails during scan");
    for failure in failures {
        eprintln!("[media] {failure}");
    }
    state.db.list_media(Some(MediaKind::Raw))
}

#[tauri::command]
pub fn list_media(
    state: State<'_, AppState>,
    kind: Option<MediaKind>,
) -> AppResult<Vec<MediaItem>> {
    state.db.list_media(kind)
}

#[tauri::command]
pub fn get_media_audio_tracks(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Vec<AudioTrack>> {
    let item = state.db.get_media(&id)?;
    state.ffmpeg.audio_tracks(Path::new(&item.path))
}

#[tauri::command]
pub fn get_audio_track_preview(
    state: State<'_, AppState>,
    id: String,
    stream_index: u32,
) -> AppResult<String> {
    let item = state.db.get_media(&id)?;
    let source = Path::new(&item.path);
    let tracks = state.ffmpeg.audio_tracks(source)?;
    if !tracks.iter().any(|track| track.index == stream_index) {
        return Err(user_error("audio track was not found"));
    }
    let output = state.audio_preview_dir.join(format!(
        "{}-{}-{}-{}.m4a",
        item.id, item.modified_at, item.file_size, stream_index
    ));
    state.ffmpeg.audio_preview(source, &output, stream_index)?;
    Ok(output.to_string_lossy().to_string())
}

#[tauri::command]
pub fn repair_thumbnails(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let _index_guard = state
        .media_index_lock
        .lock()
        .map_err(|_| user_error("media index lock was poisoned"))?;
    let (repaired, failures) = repair_missing_thumbnails(&state)?;
    eprintln!("[media] repaired {repaired} missing thumbnails");
    for failure in failures {
        eprintln!("[media] {failure}");
    }
    state.db.list_media(None)
}

#[tauri::command]
pub fn update_media_tags(
    state: State<'_, AppState>,
    id: String,
    tags: Vec<String>,
) -> AppResult<MediaItem> {
    let normalized = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    state.db.update_tags(&id, &normalized)?;
    state.db.get_media(&id)
}

#[tauri::command]
pub fn update_media_notes(
    state: State<'_, AppState>,
    id: String,
    notes: Option<String>,
) -> AppResult<MediaItem> {
    let cleaned = notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    state.db.update_notes(&id, cleaned)?;
    state.db.get_media(&id)
}

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> AppResult<Vec<Collection>> {
    state.db.list_collections()
}

#[tauri::command]
pub fn list_collection_media(
    state: State<'_, AppState>,
    collection_id: String,
) -> AppResult<Vec<MediaItem>> {
    state.db.list_collection_media(&collection_id)
}

#[tauri::command]
pub fn create_collection(state: State<'_, AppState>, name: String) -> AppResult<Vec<Collection>> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return Err(user_error("collection name cannot be empty"));
    }
    state
        .db
        .create_collection(&stable_id(cleaned), cleaned, Utc::now().timestamp())?;
    state.db.list_collections()
}

#[tauri::command]
pub fn add_to_collection(
    state: State<'_, AppState>,
    collection_id: String,
    media_id: String,
) -> AppResult<Vec<Collection>> {
    state
        .db
        .add_to_collection(&collection_id, &media_id, Utc::now().timestamp())?;
    state.db.list_collections()
}

#[tauri::command]
pub fn create_clip(state: State<'_, AppState>, request: CreateClipRequest) -> AppResult<MediaItem> {
    let settings = state.db.load_settings()?;
    let source = state.db.get_media(&request.source_id)?;
    if source.kind != MediaKind::Raw {
        return Err(user_error("clips must be created from raw media"));
    }
    if !request.start_seconds.is_finite()
        || !request.end_seconds.is_finite()
        || request.start_seconds < 0.0
        || request.end_seconds <= request.start_seconds
    {
        return Err(user_error("trim range is invalid"));
    }
    if source.duration_seconds > 0.0 && request.end_seconds > source.duration_seconds + 0.05 {
        return Err(user_error("trim end exceeds the source duration"));
    }
    if !Path::new(&source.path).is_file() {
        return Err(user_error("source file is no longer available"));
    }
    let audio_tracks = state.ffmpeg.audio_tracks(Path::new(&source.path))?;
    let valid_indices = audio_tracks
        .iter()
        .map(|track| track.index)
        .collect::<HashSet<_>>();
    let mut selected_tracks = request
        .enabled_audio_track_indices
        .clone()
        .unwrap_or_else(|| audio_tracks.iter().map(|track| track.index).collect());
    selected_tracks.sort_unstable();
    selected_tracks.dedup();
    if selected_tracks
        .iter()
        .any(|index| !valid_indices.contains(index))
    {
        return Err(user_error("one or more selected audio tracks are invalid"));
    }
    let request = CreateClipRequest {
        enabled_audio_track_indices: Some(selected_tracks),
        ..request
    };
    let clip_dir = resolve_clip_dir(&settings, &state.data_dir)?;
    let output_path = media::clip_output_path(&clip_dir, &request, &source);
    let item = media::build_clip_item(
        &request,
        &source,
        output_path,
        &state.ffmpeg,
        &state.thumbnail_dir,
    )?;
    if let Err(error) = state.db.upsert_media(&item) {
        let _ = std::fs::remove_file(&item.path);
        if let Some(thumbnail) = &item.thumbnail_path {
            let _ = std::fs::remove_file(thumbnail);
        }
        return Err(error);
    }
    Ok(item)
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> AppResult<()> {
    explorer::reveal(Path::new(&path))
}

#[tauri::command]
pub fn get_storage_stats(state: State<'_, AppState>) -> AppResult<StorageStats> {
    let settings = state.db.load_settings()?;
    let media = state.db.list_media(None)?;
    Ok(storage::stats(&media, &settings))
}

#[tauri::command]
pub fn get_cleanup_candidates(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let media = state.db.list_media(Some(MediaKind::Raw))?;
    Ok(storage::cleanup_candidates(&media))
}

#[tauri::command]
pub fn delete_raw_file(state: State<'_, AppState>, id: String) -> AppResult<Vec<MediaItem>> {
    let item = state.db.get_media(&id)?;
    if item.kind != MediaKind::Raw {
        return Err(user_error("only raw files can be deleted by cleanup"));
    }
    let path = PathBuf::from(&item.path);
    let settings = state.db.load_settings()?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| user_error("raw file is no longer available"))?;
    let source_roots = settings
        .source_folders
        .iter()
        .filter_map(|source| Path::new(source).canonicalize().ok())
        .collect::<Vec<_>>();
    if !belongs_to_roots(&canonical_path, &source_roots)
        || belongs_to_roots(&canonical_path, &output_roots(&settings))
    {
        return Err(user_error(
            "cleanup refused a file outside the configured raw source folders",
        ));
    }
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    state.db.delete_media(&id)?;
    if let Some(thumbnail) = item.thumbnail_path {
        let thumbnail = PathBuf::from(thumbnail);
        if thumbnail.starts_with(&state.thumbnail_dir) && thumbnail.is_file() {
            let _ = std::fs::remove_file(thumbnail);
        }
    }
    state.db.list_media(Some(MediaKind::Raw))
}

fn resolve_clip_dir(settings: &AppSettings, data_dir: &Path) -> AppResult<PathBuf> {
    let clip_dir = settings
        .clip_folder
        .as_ref()
        .or(settings.working_folder.as_ref())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("clips"));
    std::fs::create_dir_all(&clip_dir)?;
    Ok(clip_dir)
}

fn validate_settings(settings: &AppSettings) -> AppResult<()> {
    if !(50..=99).contains(&settings.cleanup_threshold_percent) {
        return Err(user_error(
            "cleanup threshold must be between 50 and 99 percent",
        ));
    }
    if settings.storage_limit_bytes == Some(0) {
        return Err(user_error("storage limit must be greater than zero"));
    }
    if settings
        .source_folders
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err(user_error("source folders cannot contain an empty path"));
    }
    let outputs = output_roots(settings);
    for source in &settings.source_folders {
        let source_path = Path::new(source);
        if outputs
            .iter()
            .any(|output| path_is_within(output, source_path))
        {
            return Err(user_error(
                "clip output cannot be inside a source folder; choose a separate clip or working folder",
            ));
        }
    }
    Ok(())
}

fn output_roots(settings: &AppSettings) -> Vec<PathBuf> {
    settings
        .clip_folder
        .as_ref()
        .or(settings.working_folder.as_ref())
        .map(PathBuf::from)
        .into_iter()
        .collect()
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path_key = media::normalized_path_key(path);
    let root_key = media::normalized_path_key(root);
    path_key == root_key || path_key.starts_with(&format!("{root_key}\\"))
}

fn belongs_to_sources(path: &Path, sources: &[String]) -> bool {
    sources
        .iter()
        .any(|source| path_is_within(path, Path::new(source)))
}

fn belongs_to_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path_is_within(path, root))
}

fn repair_missing_thumbnails(state: &State<'_, AppState>) -> AppResult<(usize, Vec<String>)> {
    let items = state.db.list_media(None)?;
    let mut repaired = 0;
    let mut failures = Vec::new();
    for item in items {
        let current = item.thumbnail_path.as_ref().map(Path::new);
        let needs_thumbnail = current
            .and_then(|path| path.metadata().ok())
            .map(|metadata| !metadata.is_file() || metadata.len() <= 1024)
            .unwrap_or(true);

        if !needs_thumbnail {
            continue;
        }

        match media::ensure_thumbnail(
            &item.id,
            Path::new(&item.path),
            item.duration_seconds,
            &state.ffmpeg,
            &state.thumbnail_dir,
        ) {
            Ok(Some(thumbnail_path)) => {
                let thumbnail = thumbnail_path.to_string_lossy().to_string();
                state.db.update_thumbnail(&item.id, Some(&thumbnail))?;
                repaired += 1;
            }
            Ok(None) => {}
            Err(error) => {
                failures.push(format!(
                    "thumbnail generation failed for {}: {error}",
                    item.filename
                ));
            }
        }
    }
    Ok((repaired, failures))
}

fn stable_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}
