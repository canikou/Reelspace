use crate::error::{user_error, AppResult};
use crate::explorer;
use crate::media;
use crate::models::{
    AppSettings, AppSnapshot, AudioPreviewResult, AudioTrack, Collection, CreateClipRequest,
    MediaItem, MediaKind, StorageStats,
};
use crate::state::AppState;
use crate::storage;
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::State;

async fn run_media_job<T, F>(
    state: &AppState,
    operation: &str,
    fields: Value,
    cancellation: Option<Arc<AtomicBool>>,
    task: F,
) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    let queue = match operation {
        "clip_export" => state.export_jobs.clone(),
        "library_scan" | "thumbnail_repair" => state.background_jobs.clone(),
        _ => state.media_jobs.clone(),
    };
    let telemetry = state.telemetry.clone();
    let job_id = queue.next_id(operation);
    telemetry.event(
        "info",
        "media.job.queued",
        json!({ "jobId": job_id, "operation": operation, "context": fields }),
    );
    let job_operation = operation.to_string();
    let job_fields = fields.clone();
    let task_result = tauri::async_runtime::spawn_blocking(move || {
        let Some((_permit, queue_wait, active_jobs)) = queue.acquire(cancellation.as_deref())
        else {
            telemetry.event(
                "info",
                "media.job.cancelled",
                json!({ "jobId": job_id, "operation": job_operation, "phase": "queued" }),
            );
            return Err(user_error("media job cancelled"));
        };
        telemetry.event(
            "info",
            "media.job.started",
            json!({
                "jobId": job_id,
                "operation": job_operation,
                "queueWaitMs": queue_wait.as_millis() as u64,
                "activeJobs": active_jobs,
                "context": job_fields,
            }),
        );
        let started = Instant::now();
        let result = task();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(_) => telemetry.event(
                "info",
                "media.job.finished",
                json!({
                    "jobId": job_id,
                    "operation": job_operation,
                    "status": "ok",
                    "elapsedMs": elapsed_ms,
                }),
            ),
            Err(error) => telemetry.event(
                "error",
                "media.job.finished",
                json!({
                    "jobId": job_id,
                    "operation": job_operation,
                    "status": "error",
                    "elapsedMs": elapsed_ms,
                    "error": error.to_string(),
                }),
            ),
        }
        result
    })
    .await
    .map_err(|error| user_error(format!("{operation} worker failed: {error}")))?;
    task_result
}

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
pub async fn scan_sources(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    run_media_job(&runtime, "library_scan", json!({}), None, move || {
        scan_sources_inner(&job_runtime)
    })
    .await
}

fn scan_sources_inner(state: &AppState) -> AppResult<Vec<MediaItem>> {
    let _index_guard = state
        .media_index_lock
        .lock()
        .map_err(|_| user_error("media index lock was poisoned"))?;
    let started = Instant::now();
    state
        .telemetry
        .event("info", "library.scan.started", json!({}));
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
    let scanned_count = scanned.len();
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
    let (repaired, failures) = repair_missing_thumbnails(state)?;
    for failure in &failures {
        state.telemetry.event(
            "warn",
            "thumbnail.generate.failed",
            json!({ "error": failure }),
        );
    }
    state.telemetry.event(
        "info",
        "library.scan.finished",
        json!({
            "indexedFiles": scanned_count,
            "repairedThumbnails": repaired,
            "thumbnailFailures": failures.len(),
            "elapsedMs": started.elapsed().as_millis() as u64,
        }),
    );
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
pub async fn get_media_audio_tracks(
    state: State<'_, AppState>,
    id: String,
    session_id: String,
) -> AppResult<Vec<AudioTrack>> {
    let item = state.db.get_media(&id)?;
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    let cancellation = runtime.preview_coordinator.session(&session_id);
    let path = PathBuf::from(item.path);
    run_media_job(
        &runtime,
        "audio_probe",
        json!({
            "mediaId": id,
            "sessionId": session_id,
            "sourceBytes": item.file_size,
            "sourceDurationSeconds": item.duration_seconds,
        }),
        Some(Arc::clone(&cancellation)),
        move || {
            if cancellation.load(Ordering::Acquire) {
                return Err(user_error("audio probe cancelled"));
            }
            job_runtime
                .ffmpeg
                .audio_tracks_cancellable(&path, &cancellation)
        },
    )
    .await
}

#[tauri::command]
pub async fn get_audio_track_preview(
    state: State<'_, AppState>,
    id: String,
    stream_index: u32,
    session_id: String,
) -> AppResult<AudioPreviewResult> {
    let item = state.db.get_media(&id)?;
    let output = state.audio_preview_dir.join(format!(
        "{}-{}-{}-{}.m4a",
        item.id, item.modified_at, item.file_size, stream_index
    ));
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    let cancellation = runtime.preview_coordinator.session(&session_id);
    let queue_cancellation = Arc::clone(&cancellation);
    let source = PathBuf::from(item.path);
    let media_id = item.id.clone();
    run_media_job(
        &runtime,
        "audio_preview",
        json!({
            "mediaId": media_id,
            "sessionId": session_id,
            "streamIndex": stream_index,
            "sourceBytes": item.file_size,
            "sourceDurationSeconds": item.duration_seconds,
        }),
        Some(queue_cancellation),
        move || {
            let started = Instant::now();
            if cancellation.load(Ordering::Acquire) {
                return Err(user_error("audio preview cancelled"));
            }
            let output_lock = job_runtime.preview_coordinator.output_lock(&output);
            let output_guard = crate::jobs::lock_unpoisoned(&output_lock);
            let generated =
                job_runtime
                    .ffmpeg
                    .audio_preview(&source, &output, stream_index, &cancellation);
            drop(output_guard);
            let method = generated?;
            let bytes = output
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            prune_audio_preview_cache(&job_runtime.audio_preview_dir, &output, &job_runtime);
            let result = AudioPreviewResult {
                path: output.to_string_lossy().to_string(),
                cache_hit: matches!(method, crate::ffmpeg::AudioPreviewMethod::Cache),
                method: method.as_str().to_string(),
                elapsed_ms: started.elapsed().as_millis() as u64,
                bytes,
            };
            job_runtime.telemetry.event(
                "info",
                "audio.preview.ready",
                json!({
                    "origin": "backend",
                    "mediaId": media_id,
                    "sessionId": session_id,
                    "streamIndex": stream_index,
                    "method": result.method,
                    "cacheHit": result.cache_hit,
                    "elapsedMs": result.elapsed_ms,
                    "outputBytes": result.bytes,
                }),
            );
            Ok(result)
        },
    )
    .await
}

#[tauri::command]
pub fn cancel_audio_preview_session(state: State<'_, AppState>, session_id: String) {
    let cancelled = state.preview_coordinator.cancel_session(&session_id);
    state.telemetry.event(
        "info",
        "audio.preview.session_cancel",
        json!({ "sessionId": session_id, "active": cancelled }),
    );
}

#[tauri::command]
pub fn log_client_event(
    state: State<'_, AppState>,
    event: String,
    mut fields: Value,
) -> AppResult<()> {
    let valid = !event.is_empty()
        && event.len() <= 80
        && event
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character));
    if !valid {
        return Err(user_error("telemetry event name is invalid"));
    }
    if !fields.is_object() || serde_json::to_vec(&fields)?.len() > 16 * 1024 {
        return Err(user_error("telemetry fields are invalid"));
    }
    if let Some(fields) = fields.as_object_mut() {
        fields.insert("origin".to_string(), json!("client"));
    }
    state.telemetry.event("info", &event, fields);
    Ok(())
}

#[tauri::command]
pub async fn repair_thumbnails(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    run_media_job(&runtime, "thumbnail_repair", json!({}), None, move || {
        repair_thumbnails_inner(&job_runtime)
    })
    .await
}

fn repair_thumbnails_inner(state: &AppState) -> AppResult<Vec<MediaItem>> {
    let _index_guard = state
        .media_index_lock
        .lock()
        .map_err(|_| user_error("media index lock was poisoned"))?;
    let (repaired, failures) = repair_missing_thumbnails(state)?;
    state.telemetry.event(
        "info",
        "thumbnail.repair.finished",
        json!({ "repaired": repaired, "failures": failures.len() }),
    );
    for failure in failures {
        state.telemetry.event(
            "warn",
            "thumbnail.generate.failed",
            json!({ "error": failure }),
        );
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
pub async fn create_clip(
    state: State<'_, AppState>,
    request: CreateClipRequest,
) -> AppResult<MediaItem> {
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    let fields = json!({
        "mediaId": request.source_id,
        "sessionId": request.session_id,
        "startSeconds": request.start_seconds,
        "endSeconds": request.end_seconds,
        "audioTrackCount": request.enabled_audio_track_indices.as_ref().map(Vec::len),
    });
    run_media_job(&runtime, "clip_export", fields, None, move || {
        create_clip_inner(&job_runtime, request)
    })
    .await
}

fn create_clip_inner(state: &AppState, request: CreateClipRequest) -> AppResult<MediaItem> {
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
    let (item, trim_method) = media::build_clip_item(
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
    state.telemetry.event(
        "info",
        "clip.export.ready",
        json!({
            "origin": "backend",
            "mediaId": item.id,
            "sessionId": request.session_id,
            "encoder": trim_method.as_str(),
            "outputBytes": item.file_size,
            "outputDurationSeconds": item.duration_seconds,
            "audioTrackCount": request.enabled_audio_track_indices.as_ref().map(Vec::len).unwrap_or(0),
        }),
    );
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
    remove_audio_previews_for_media(&state.audio_preview_dir, &id, &state);
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

fn repair_missing_thumbnails(state: &AppState) -> AppResult<(usize, Vec<String>)> {
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

fn prune_audio_preview_cache(cache_dir: &Path, keep: &Path, state: &AppState) {
    const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
    const TARGET_CACHE_BYTES: u64 = 400 * 1024 * 1024;
    const STALE_PART_AGE: Duration = Duration::from_secs(24 * 60 * 60);

    let Ok(_cleanup_guard) = state.audio_cache_cleanup_lock.try_lock() else {
        return;
    };
    let keep_name = keep
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let media_prefix = keep_name.get(..64).unwrap_or("");
    let stream_suffix = keep_name
        .strip_suffix(".m4a")
        .and_then(|name| name.rsplit_once('-'))
        .map(|(_, stream)| format!("-{stream}.m4a"));
    let now = SystemTime::now();
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    let mut cached = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file() && !file_type.is_symlink())
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || path == keep {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let stale_part = name.contains(".part.m4a")
            && now.duration_since(modified).unwrap_or_default() > STALE_PART_AGE;
        let stale_version = !media_prefix.is_empty()
            && name.starts_with(media_prefix)
            && stream_suffix
                .as_ref()
                .is_some_and(|suffix| name.ends_with(suffix));
        if stale_part || stale_version {
            match fs::remove_file(&path) {
                Ok(()) => state.telemetry.event(
                    "info",
                    "audio.cache.evicted",
                    json!({ "reason": if stale_part { "stale_part" } else { "stale_version" }, "bytes": metadata.len() }),
                ),
                Err(error) => state.telemetry.event(
                    "warn",
                    "audio.cache.evict_failed",
                    json!({ "error": error.to_string() }),
                ),
            }
            continue;
        }
        if name.ends_with(".m4a") {
            cached.push((path, metadata.len(), modified));
        }
    }

    let keep_bytes = keep.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut total_bytes = cached.iter().map(|(_, bytes, _)| *bytes).sum::<u64>() + keep_bytes;
    if total_bytes <= MAX_CACHE_BYTES {
        return;
    }
    cached.sort_by_key(|(_, _, modified)| *modified);
    for (path, bytes, _) in cached {
        if total_bytes <= TARGET_CACHE_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total_bytes = total_bytes.saturating_sub(bytes);
            state.telemetry.event(
                "info",
                "audio.cache.evicted",
                json!({ "reason": "size_limit", "bytes": bytes, "remainingBytes": total_bytes }),
            );
        }
    }
}

fn remove_audio_previews_for_media(cache_dir: &Path, media_id: &str, state: &AppState) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(media_id) && name.ends_with(".m4a") {
            if let Ok(metadata) = entry.metadata() {
                if fs::remove_file(entry.path()).is_ok() {
                    state.telemetry.event(
                        "info",
                        "audio.cache.evicted",
                        json!({ "reason": "media_deleted", "bytes": metadata.len(), "mediaId": media_id }),
                    );
                }
            }
        }
    }
}

fn stable_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}
