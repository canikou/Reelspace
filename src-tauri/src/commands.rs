use crate::error::{user_error, AppResult};
use crate::explorer;
use crate::media;
use crate::models::{AppSettings, AppSnapshot, Collection, CreateClipRequest, MediaItem, MediaKind, StorageStats};
use crate::state::AppState;
use crate::storage;
use chrono::Utc;
use sha2::{Digest, Sha256};
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
    state.db.save_settings(&settings)?;
    get_app_state(state)
}

#[tauri::command]
pub fn scan_sources(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let settings = state.db.load_settings()?;
    for folder in &settings.source_folders {
        let items = media::scan_folder(Path::new(folder), &state.ffmpeg, &state.thumbnail_dir)?;
        for item in items {
            state.db.upsert_media(&item)?;
        }
    }
    state.db.list_media(Some(MediaKind::Raw))
}

#[tauri::command]
pub fn list_media(state: State<'_, AppState>, kind: Option<MediaKind>) -> AppResult<Vec<MediaItem>> {
    state.db.list_media(kind)
}

#[tauri::command]
pub fn update_media_tags(state: State<'_, AppState>, id: String, tags: Vec<String>) -> AppResult<MediaItem> {
    let normalized = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    state.db.update_tags(&id, &normalized)?;
    state.db.get_media(&id)
}

#[tauri::command]
pub fn update_media_notes(state: State<'_, AppState>, id: String, notes: Option<String>) -> AppResult<MediaItem> {
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
    let clip_dir = resolve_clip_dir(&settings, &state.data_dir)?;
    let output_path = media::clip_output_path(&clip_dir, &request, &source);
    let item = media::build_clip_item(&request, &source, output_path, &state.ffmpeg, &state.thumbnail_dir)?;
    state.db.upsert_media(&item)?;
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
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    state.db.delete_media(&id)?;
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

fn stable_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}
