use crate::error::{app_error, user_error, AppError, AppResult, ErrorCode};
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
            return Err(AppError::cancelled("Media job"));
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
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)).unwrap_or_else(|_| {
                Err(app_error(
                    ErrorCode::Internal,
                    "The media operation failed in isolation and can be retried",
                ))
            });
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
    .map_err(|error| {
        app_error(ErrorCode::Internal, format!("{operation} worker failed"))
            .with_detail(error.to_string())
    })?;
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
    state.authorize_settings_assets(&settings)?;
    state.db.save_settings(&settings)?;
    get_app_state(state)
}

#[tauri::command]
pub async fn scan_sources(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let runtime = state.inner().clone();
    if runtime
        .scan_in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        runtime
            .scan_coalesced_events
            .fetch_add(1, Ordering::Relaxed);
        runtime.telemetry.event(
            "info",
            "library.scan.coalesced",
            json!({ "reason": "scan_in_flight" }),
        );
        return runtime.db.list_media(Some(MediaKind::Raw));
    }
    let _scan_guard = ScanInFlightGuard(Arc::clone(&runtime.scan_in_flight));
    let job_runtime = runtime.clone();
    let result = run_media_job(&runtime, "library_scan", json!({}), None, move || {
        scan_sources_inner(&job_runtime)
    })
    .await?;
    start_thumbnail_worker(runtime);
    Ok(result)
}

struct ScanInFlightGuard(Arc<AtomicBool>);

impl Drop for ScanInFlightGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
    let mut reachable_sources = Vec::new();
    let mut files_visited = 0;
    let mut files_changed = 0;
    let mut files_reprobed = 0;
    let mut cached_probe_hits = 0;
    let mut files_skipped = 0;
    let mut files_missing = 0;
    let mut traversal_errors = 0;
    let mut changed_path_keys = HashSet::new();
    let mut failures_by_code = HashMap::<String, usize>::new();
    for folder in &settings.source_folders {
        let source_id = stable_id(folder);
        match media::scan_folder(
            Path::new(folder),
            &state.ffmpeg,
            &state.db,
            &state.thumbnail_dir,
            &existing,
            &excluded_roots,
        ) {
            Ok(result) => {
                if result.traversal_errors == 0 {
                    reachable_sources.push(PathBuf::from(folder));
                }
                files_visited += result.files_visited;
                files_changed += result.files_changed;
                files_reprobed += result.files_reprobed;
                cached_probe_hits += result.cached_probe_hits;
                files_skipped += result.files_skipped;
                traversal_errors += result.traversal_errors;
                changed_path_keys.extend(result.changed_path_keys);
                for (code, count) in result.failures_by_code {
                    *failures_by_code.entry(code).or_default() += count;
                }
                scanned.extend(result.items);
            }
            Err(error) if error.code == ErrorCode::SourceUnavailable => {
                state.telemetry.event(
                    "warn",
                    "library.source.unavailable",
                    json!({ "sourceId": source_id, "code": error.code }),
                );
            }
            Err(error) => {
                state.telemetry.event(
                    "error",
                    "library.source.failed",
                    json!({ "sourceId": source_id, "code": error.code }),
                );
            }
        }
    }
    let scanned_count = scanned.len();
    let changed_items = scanned
        .iter()
        .filter(|item| {
            let key = media::normalized_path_key(Path::new(&item.path));
            changed_path_keys.contains(&key) || !existing.contains_key(&key)
        })
        .cloned()
        .collect::<Vec<_>>();
    state.db.upsert_media_batch(&changed_items)?;
    for item in existing_items {
        let path = Path::new(&item.path);
        if belongs_to_roots(path, &reachable_sources)
            && !belongs_to_roots(path, &excluded_roots)
            && matches!(path.try_exists(), Ok(false))
        {
            state.db.delete_media(&item.id)?;
            files_missing += 1;
        }
    }
    let queued_thumbnails = queue_missing_thumbnails(state, 0, false)?;
    state.telemetry.event(
        "info",
        "library.scan.finished",
        json!({
            "indexedFiles": scanned_count,
            "sourcesConfigured": settings.source_folders.len(),
            "sourcesReached": reachable_sources.len(),
            "filesVisited": files_visited,
            "filesChanged": files_changed,
            "filesMissing": files_missing,
            "filesReprobed": files_reprobed,
            "cachedProbeHits": cached_probe_hits,
            "filesSkipped": files_skipped,
            "traversalErrors": traversal_errors,
            "coalescedEvents": state.scan_coalesced_events.swap(0, Ordering::AcqRel),
            "probeCacheEntries": state.db.probe_cache_count()?,
            "queuedThumbnails": queued_thumbnails,
            "pendingThumbnailJobs": state.db.thumbnail_job_count()?,
            "failuresByCode": failures_by_code,
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
    validate_id(&id, "media")?;
    validate_session_id(&session_id)?;
    let item = state.db.get_media(&id)?;
    let path = PathBuf::from(&item.path);
    let fingerprint = media::FileFingerprint::read(&path)?;
    if let Some(cached) = state.db.get_probe_cache(&fingerprint)? {
        state.telemetry.event(
            "info",
            "media.probe.cache_hit",
            json!({ "mediaId": id, "operation": "audio_tracks" }),
        );
        return Ok(cached.audio_tracks);
    }
    let runtime = state.inner().clone();
    let job_runtime = runtime.clone();
    let cancellation = runtime.preview_coordinator.session(&session_id);
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
                return Err(AppError::cancelled("Audio probe"));
            }
            let probed = job_runtime
                .ffmpeg
                .probe_data_cancellable(&path, &cancellation)?;
            job_runtime.db.put_probe_cache(&fingerprint, &probed)?;
            Ok(probed.audio_tracks)
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
    validate_id(&id, "media")?;
    validate_session_id(&session_id)?;
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
                return Err(AppError::cancelled("Audio preview"));
            }
            let fingerprint = media::FileFingerprint::read(&source)?;
            let probe = match job_runtime.db.get_probe_cache(&fingerprint)? {
                Some(probe) => probe,
                None => {
                    let probe = job_runtime
                        .ffmpeg
                        .probe_data_cancellable(&source, &cancellation)?;
                    job_runtime.db.put_probe_cache(&fingerprint, &probe)?;
                    probe
                }
            };
            validate_audio_track_index(stream_index, &probe.audio_tracks)?;
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
pub fn cancel_audio_preview_session(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<()> {
    validate_session_id(&session_id)?;
    let cancelled = state.preview_coordinator.cancel_session(&session_id);
    state.telemetry.event(
        "info",
        "audio.preview.session_cancel",
        json!({ "sessionId": session_id, "active": cancelled }),
    );
    Ok(())
}

#[tauri::command]
pub fn log_client_event(
    state: State<'_, AppState>,
    event: String,
    mut fields: Value,
) -> AppResult<()> {
    validate_client_event(&event, &fields)?;
    if let Some(fields) = fields.as_object_mut() {
        fields.insert("origin".to_string(), json!("client"));
    }
    state.telemetry.event("info", &event, fields);
    Ok(())
}

#[tauri::command]
pub fn repair_thumbnails(state: State<'_, AppState>) -> AppResult<Vec<MediaItem>> {
    let runtime = state.inner().clone();
    let queued = queue_missing_thumbnails(&runtime, 100, true)?;
    runtime.telemetry.event(
        "info",
        "thumbnail.repair.queued",
        json!({ "queued": queued }),
    );
    start_thumbnail_worker(runtime.clone());
    runtime.db.list_media(None)
}

#[tauri::command]
pub fn update_media_tags(
    state: State<'_, AppState>,
    id: String,
    tags: Vec<String>,
) -> AppResult<MediaItem> {
    validate_id(&id, "media")?;
    let normalized = normalize_tags(tags)?;
    state.db.update_tags(&id, &normalized)?;
    state.db.get_media(&id)
}

#[tauri::command]
pub fn update_media_notes(
    state: State<'_, AppState>,
    id: String,
    notes: Option<String>,
) -> AppResult<MediaItem> {
    validate_id(&id, "media")?;
    let cleaned = normalize_notes(notes)?;
    state.db.update_notes(&id, cleaned.as_deref())?;
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
    validate_id(&collection_id, "collection")?;
    state.db.list_collection_media(&collection_id)
}

#[tauri::command]
pub fn create_collection(state: State<'_, AppState>, name: String) -> AppResult<Vec<Collection>> {
    let cleaned = normalize_collection_name(&name)?;
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
    validate_id(&collection_id, "collection")?;
    validate_id(&media_id, "media")?;
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

fn create_clip_inner(state: &AppState, mut request: CreateClipRequest) -> AppResult<MediaItem> {
    validate_id(&request.source_id, "media")?;
    if let Some(session_id) = request.session_id.as_deref() {
        validate_session_id(session_id)?;
    }
    request.title = normalize_clip_title(request.title)?;
    if request
        .enabled_audio_track_indices
        .as_ref()
        .is_some_and(|tracks| tracks.len() > 32)
    {
        return Err(user_error("too many audio tracks were selected"));
    }
    let settings = state.db.load_settings()?;
    let source = state.db.get_media(&request.source_id)?;
    if source.kind != MediaKind::Raw {
        return Err(app_error(
            ErrorCode::InvalidRequest,
            "Clips must be created from raw media",
        ));
    }
    validate_trim_range(
        request.start_seconds,
        request.end_seconds,
        source.duration_seconds,
    )?;
    if !Path::new(&source.path).is_file() {
        return Err(app_error(
            ErrorCode::SourceUnavailable,
            "The source recording is no longer available",
        ));
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
pub fn reveal_in_explorer(state: State<'_, AppState>, id: String) -> AppResult<()> {
    validate_id(&id, "media")?;
    let item = state.db.get_media(&id)?;
    explorer::reveal(Path::new(&item.path))
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
    validate_id(&id, "media")?;
    let item = state.db.get_media(&id)?;
    let settings = state.db.load_settings()?;
    let path = validated_raw_deletion_path(&item, &settings)?;
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

fn validated_raw_deletion_path(item: &MediaItem, settings: &AppSettings) -> AppResult<PathBuf> {
    if item.kind != MediaKind::Raw {
        return Err(app_error(
            ErrorCode::InvalidRequest,
            "Only raw recordings can be deleted by cleanup",
        ));
    }
    let path = PathBuf::from(&item.path);
    let canonical_path = path.canonicalize().map_err(|error| {
        app_error(
            ErrorCode::SourceUnavailable,
            "The raw recording is no longer available",
        )
        .with_detail(error.to_string())
    })?;
    let source_roots = settings
        .source_folders
        .iter()
        .filter_map(|source| Path::new(source).canonicalize().ok())
        .collect::<Vec<_>>();
    if !belongs_to_roots(&canonical_path, &source_roots)
        || belongs_to_roots(&canonical_path, &output_roots(settings))
    {
        return Err(app_error(
            ErrorCode::PathOutsideManagedRoots,
            "Cleanup refused a file outside the configured raw source folders",
        ));
    }
    Ok(canonical_path)
}

fn validate_trim_range(start: f64, end: f64, duration: f64) -> AppResult<()> {
    if !start.is_finite()
        || !end.is_finite()
        || !duration.is_finite()
        || duration <= 0.0
        || start < 0.0
        || end <= start
    {
        return Err(user_error("trim range is invalid"));
    }
    if duration > 0.0 && end > duration + 0.05 {
        return Err(user_error("trim end exceeds the source duration"));
    }
    Ok(())
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
    if settings.source_folders.iter().any(|value| {
        value.trim().is_empty() || value != value.trim() || contains_forbidden_control(value, false)
    }) {
        return Err(user_error("source folders cannot contain an empty path"));
    }
    if settings.source_folders.len() > 64 {
        return Err(user_error(
            "no more than 64 source folders can be configured",
        ));
    }
    for path in settings
        .source_folders
        .iter()
        .map(String::as_str)
        .chain(settings.working_folder.as_deref())
        .chain(settings.clip_folder.as_deref())
    {
        let lower = path.to_ascii_lowercase();
        if path.trim().is_empty()
            || path != path.trim()
            || path.len() > 32_767
            || contains_forbidden_control(path, false)
            || lower.starts_with("\\\\.\\")
            || lower.starts_with("\\\\?\\globalroot")
            || !Path::new(path).is_absolute()
        {
            return Err(app_error(
                ErrorCode::InvalidPath,
                "Configured folders must use absolute Windows paths",
            ));
        }
    }
    let mut unique_sources = HashSet::new();
    if settings
        .source_folders
        .iter()
        .any(|source| !unique_sources.insert(media::normalized_path_key(Path::new(source))))
    {
        return Err(user_error("source folders cannot contain duplicates"));
    }
    for (index, source) in settings.source_folders.iter().enumerate() {
        if settings.source_folders.iter().skip(index + 1).any(|other| {
            path_is_within(Path::new(source), Path::new(other))
                || path_is_within(Path::new(other), Path::new(source))
        }) {
            return Err(user_error(
                "source folders cannot overlap; configure only the common parent folder",
            ));
        }
    }
    let outputs = output_roots(settings);
    for source in &settings.source_folders {
        let source_path = Path::new(source);
        if outputs.iter().any(|output| {
            path_is_within(output, source_path) || path_is_within(source_path, output)
        }) {
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

fn belongs_to_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path_is_within(path, root))
}

fn queue_missing_thumbnails(
    state: &AppState,
    priority: i64,
    force_retry: bool,
) -> AppResult<usize> {
    let items = state.db.list_media(None)?;
    let mut queued = 0;
    for item in items {
        let current = item.thumbnail_path.as_ref().map(Path::new);
        let needs_thumbnail = current
            .and_then(|path| path.metadata().ok())
            .map(|metadata| !metadata.is_file() || metadata.len() <= 1024)
            .unwrap_or(true);

        if !needs_thumbnail {
            continue;
        }

        if let Ok(fingerprint) = media::FileFingerprint::read(Path::new(&item.path)) {
            state
                .db
                .queue_thumbnail_job(&item, &fingerprint, priority, force_retry)?;
            queued += 1;
        }
    }
    if queued > 0 {
        state.thumbnail_scheduler.notify_work();
    }
    Ok(queued)
}

pub(crate) fn start_thumbnail_worker(state: AppState) {
    if !state.thumbnail_scheduler.try_start() {
        return;
    }
    let scheduler = Arc::clone(&state.thumbnail_scheduler);
    tauri::async_runtime::spawn(async move {
        let worker_state = state.clone();
        let outcome =
            tauri::async_runtime::spawn_blocking(move || run_thumbnail_worker(&worker_state)).await;
        let completed_normally = matches!(outcome, Ok(Ok(())));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => state.telemetry.event(
                "error",
                "thumbnail.worker.failed",
                json!({ "code": error.code, "error": error.to_string() }),
            ),
            Err(error) => state.telemetry.event(
                "error",
                "thumbnail.worker.panicked",
                json!({ "error": error.to_string() }),
            ),
        }
        scheduler.finish();
        if completed_normally
            && !scheduler.cancelled().load(Ordering::Acquire)
            && state.db.thumbnail_job_count().unwrap_or(0) > 0
        {
            start_thumbnail_worker(state);
        }
    });
}

fn run_thumbnail_worker(state: &AppState) -> AppResult<()> {
    state.telemetry.event(
        "info",
        "thumbnail.worker.started",
        json!({ "concurrency": 1 }),
    );
    let mut completed = 0_u64;
    let mut failed = 0_u64;
    while !state
        .thumbnail_scheduler
        .cancelled()
        .load(Ordering::Acquire)
    {
        let Some(job) = state.db.claim_thumbnail_job()? else {
            let Some(delay) = state.db.next_thumbnail_job_delay()? else {
                break;
            };
            let delay = delay
                .max(Duration::from_millis(10))
                .min(Duration::from_secs(60));
            if !state.thumbnail_scheduler.wait_for_work(delay) {
                break;
            }
            continue;
        };
        let item = match state.db.get_media(&job.media_id) {
            Ok(item) => item,
            Err(error) if error.code == ErrorCode::InvalidRequest => {
                state.db.delete_thumbnail_job(&job.media_id)?;
                failed += 1;
                continue;
            }
            Err(error) => return Err(error),
        };
        let source = PathBuf::from(&job.source_path);
        let fingerprint = match media::FileFingerprint::read(&source) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                state.db.fail_thumbnail_job(
                    &job.media_id,
                    job.attempts,
                    error_code_name(error.code),
                )?;
                failed += 1;
                continue;
            }
        };
        if fingerprint.token() != job.source_fingerprint {
            state
                .db
                .queue_thumbnail_job(&item, &fingerprint, 100, false)?;
            continue;
        }
        let thumbnail_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            media::ensure_thumbnail(
                &job.media_id,
                &source,
                job.duration_seconds,
                &state.ffmpeg,
                &state.thumbnail_dir,
                Some(state.thumbnail_scheduler.cancelled()),
            )
        }))
        .unwrap_or_else(|_| {
            Err(app_error(
                ErrorCode::Internal,
                "Thumbnail generation failed in isolation and will be retried",
            ))
        });
        match thumbnail_result {
            Ok(Some(thumbnail)) => {
                state
                    .db
                    .finish_thumbnail_job(&job.media_id, &thumbnail.to_string_lossy())?;
                completed += 1;
            }
            Ok(None) => {
                state.db.fail_thumbnail_job(
                    &job.media_id,
                    job.attempts,
                    "thumbnailGenerationFailed",
                )?;
                failed += 1;
            }
            Err(error) => {
                state.db.fail_thumbnail_job(
                    &job.media_id,
                    job.attempts,
                    error_code_name(error.code),
                )?;
                failed += 1;
                state.telemetry.event(
                    if error.code == ErrorCode::MediaProcessCancelled {
                        "info"
                    } else {
                        "warn"
                    },
                    "thumbnail.generate.failed",
                    json!({
                        "mediaId": job.media_id,
                        "code": error.code,
                        "attempt": job.attempts,
                    }),
                );
            }
        }
    }
    state.telemetry.event(
        "info",
        "thumbnail.worker.finished",
        json!({ "completed": completed, "failed": failed }),
    );
    Ok(())
}

fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::FileChanged => "fileChanged",
        ErrorCode::FileNotFound => "fileNotFound",
        ErrorCode::SourceUnavailable => "sourceUnavailable",
        ErrorCode::FfmpegUnavailable => "ffmpegUnavailable",
        ErrorCode::MediaProcessCancelled => "mediaProcessCancelled",
        ErrorCode::MediaProcessTimeout => "mediaProcessTimeout",
        _ => "thumbnailGenerationFailed",
    }
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

fn validate_id(id: &str, kind: &str) -> AppResult<()> {
    if id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(user_error(format!("{kind} identifier is invalid")))
    }
}

fn validate_session_id(session_id: &str) -> AppResult<()> {
    let valid = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && session_id
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !session_id.contains("..")
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(user_error("media session identifier is invalid"))
    }
}

fn normalize_tags(tags: Vec<String>) -> AppResult<Vec<String>> {
    if tags.len() > 64 {
        return Err(user_error("a media item cannot have more than 64 tags"));
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 64 {
            return Err(user_error("tags cannot be longer than 64 characters"));
        }
        if contains_forbidden_control(tag, false) {
            return Err(user_error("tags contain unsupported control characters"));
        }
        if seen.insert(tag.to_lowercase()) {
            normalized.push(tag.to_string());
        }
    }
    if normalized.iter().map(String::len).sum::<usize>() > 4096 {
        return Err(user_error("the combined tag data is too large"));
    }
    Ok(normalized)
}

fn normalize_notes(notes: Option<String>) -> AppResult<Option<String>> {
    let Some(notes) = notes else {
        return Ok(None);
    };
    if notes.len() > 64 * 1024 {
        return Err(user_error("notes cannot be larger than 64 KB"));
    }
    if contains_forbidden_control(&notes, true) {
        return Err(user_error("notes contain unsupported control characters"));
    }
    Ok(match notes.trim() {
        "" => None,
        cleaned => Some(cleaned.to_string()),
    })
}

fn normalize_collection_name(name: &str) -> AppResult<&str> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return Err(user_error("collection name cannot be empty"));
    }
    if cleaned.chars().count() > 120 {
        return Err(user_error("collection name cannot exceed 120 characters"));
    }
    if contains_forbidden_control(cleaned, false) {
        return Err(user_error(
            "collection name contains unsupported control characters",
        ));
    }
    Ok(cleaned)
}

fn normalize_clip_title(title: Option<String>) -> AppResult<Option<String>> {
    let Some(title) = title else {
        return Ok(None);
    };
    let cleaned = title.trim();
    if cleaned.chars().count() > 120 {
        return Err(user_error("clip title cannot exceed 120 characters"));
    }
    if contains_forbidden_control(cleaned, false) {
        return Err(user_error(
            "clip title contains unsupported control characters",
        ));
    }
    Ok((!cleaned.is_empty()).then(|| cleaned.to_string()))
}

fn validate_audio_track_index(stream_index: u32, tracks: &[AudioTrack]) -> AppResult<()> {
    if tracks.iter().any(|track| track.index == stream_index) {
        Ok(())
    } else {
        Err(user_error("the selected audio track is invalid"))
    }
}

fn validate_client_event(event: &str, fields: &Value) -> AppResult<()> {
    let valid = !event.is_empty()
        && event.len() <= 80
        && event
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character));
    if !valid {
        return Err(user_error("telemetry event name is invalid"));
    }
    if !fields.is_object() || serde_json::to_vec(fields)?.len() > 16 * 1024 {
        return Err(user_error("telemetry fields are invalid"));
    }
    Ok(())
}

fn contains_forbidden_control(value: &str, allow_layout_whitespace: bool) -> bool {
    value.chars().any(|character| {
        character.is_control()
            && !(allow_layout_whitespace && matches!(character, '\n' | '\r' | '\t'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_root_checks_do_not_accept_sibling_prefixes() {
        assert!(path_is_within(
            Path::new("C:\\captures\\game\\clip.mp4"),
            Path::new("C:\\captures")
        ));
        assert!(!path_is_within(
            Path::new("C:\\captures-private\\clip.mp4"),
            Path::new("C:\\captures")
        ));
    }

    #[test]
    fn generated_outputs_cannot_be_configured_inside_raw_sources() {
        let settings = AppSettings {
            source_folders: vec!["C:\\captures".to_string()],
            clip_folder: Some("C:\\captures\\clips".to_string()),
            ..AppSettings::default()
        };
        let error = validate_settings(&settings).expect_err("nested output must fail");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn ipc_identifiers_have_a_bounded_stable_format() {
        assert!(validate_id(&"a".repeat(64), "media").is_ok());
        assert!(validate_id(&"A".repeat(64), "media").is_err());
        assert!(validate_id(&"a".repeat(63), "media").is_err());
        assert!(validate_id(&format!("{}g", "a".repeat(63)), "media").is_err());
        assert!(validate_id("../media", "media").is_err());
        assert!(validate_session_id("trim-session:42").is_ok());
        assert!(validate_session_id("bad/session").is_err());
        assert!(validate_session_id(".hidden").is_err());
        assert!(validate_session_id("trailing-").is_err());
        assert!(validate_session_id("bad..session").is_err());
        assert!(validate_session_id(&"a".repeat(129)).is_err());
    }

    #[test]
    fn invalid_trim_ranges_are_rejected() {
        assert!(validate_trim_range(f64::NAN, 2.0, 10.0).is_err());
        assert!(validate_trim_range(0.0, f64::INFINITY, 10.0).is_err());
        assert!(validate_trim_range(0.0, 1.0, f64::NAN).is_err());
        assert!(validate_trim_range(0.0, 1.0, 0.0).is_err());
        assert!(validate_trim_range(-0.001, 1.0, 10.0).is_err());
        assert!(validate_trim_range(2.0, 2.0, 10.0).is_err());
        assert!(validate_trim_range(0.0, 11.0, 10.0).is_err());
        assert!(validate_trim_range(1.0, 2.0, 10.0).is_ok());
    }

    #[test]
    fn tags_are_bounded_clean_and_case_insensitively_deduplicated() {
        assert_eq!(
            normalize_tags(vec![" Game ".into(), "game".into(), "Ranked".into()])
                .expect("valid tags"),
            vec!["Game", "Ranked"]
        );
        assert!(normalize_tags(vec!["x".repeat(65)]).is_err());
        assert!(normalize_tags(vec!["bad\0tag".into()]).is_err());
        assert!(normalize_tags((0..65).map(|index| index.to_string()).collect()).is_err());
        let wide = (0..64)
            .map(|index| format!("{index:02}{}", "\u{1f3ae}".repeat(62)))
            .collect();
        assert!(normalize_tags(wide).is_err());
    }

    #[test]
    fn settings_reject_ambiguous_duplicate_and_overlapping_paths() {
        let duplicate = AppSettings {
            source_folders: vec!["C:\\captures".into(), "c:\\captures\\".into()],
            ..AppSettings::default()
        };
        assert!(validate_settings(&duplicate).is_err());
        let source_inside_output = AppSettings {
            source_folders: vec!["C:\\managed\\raw".into()],
            clip_folder: Some("C:\\managed".into()),
            ..AppSettings::default()
        };
        assert!(validate_settings(&source_inside_output).is_err());
        let overlapping_sources = AppSettings {
            source_folders: vec!["C:\\captures".into(), "C:\\captures\\game".into()],
            ..AppSettings::default()
        };
        assert!(validate_settings(&overlapping_sources).is_err());
        let device_path = AppSettings {
            source_folders: vec!["\\\\.\\PhysicalDrive0".into()],
            ..AppSettings::default()
        };
        assert!(validate_settings(&device_path).is_err());
    }

    #[test]
    fn settings_enforce_all_numeric_and_count_boundaries() {
        for invalid in [0, 49, 100, u8::MAX] {
            let settings = AppSettings {
                cleanup_threshold_percent: invalid,
                ..AppSettings::default()
            };
            assert!(validate_settings(&settings).is_err());
        }
        for valid in [50, 99] {
            let settings = AppSettings {
                cleanup_threshold_percent: valid,
                storage_limit_bytes: Some(1),
                ..AppSettings::default()
            };
            assert!(validate_settings(&settings).is_ok());
        }
        let zero_limit = AppSettings {
            storage_limit_bytes: Some(0),
            ..AppSettings::default()
        };
        assert!(validate_settings(&zero_limit).is_err());
        let too_many = AppSettings {
            source_folders: (0..65).map(|index| format!("C:\\source-{index}")).collect(),
            ..AppSettings::default()
        };
        assert!(validate_settings(&too_many).is_err());
    }

    #[test]
    fn user_text_is_trimmed_bounded_and_control_safe() {
        assert_eq!(
            normalize_notes(Some("  note\ntext  ".into())).unwrap(),
            Some("note\ntext".into())
        );
        assert_eq!(normalize_notes(Some("  ".into())).unwrap(), None);
        assert!(normalize_notes(Some("bad\0note".into())).is_err());
        assert!(normalize_notes(Some("x".repeat(64 * 1024 + 1))).is_err());
        assert_eq!(
            normalize_collection_name("  Favorites  ").unwrap(),
            "Favorites"
        );
        assert!(normalize_collection_name("\n").is_err());
        assert!(normalize_collection_name(&"x".repeat(121)).is_err());
        assert_eq!(
            normalize_clip_title(Some("  highlight  ".into())).unwrap(),
            Some("highlight".into())
        );
        assert_eq!(normalize_clip_title(Some("  ".into())).unwrap(), None);
        assert!(normalize_clip_title(Some("bad\0title".into())).is_err());
    }

    #[test]
    fn audio_streams_and_client_telemetry_are_strictly_validated() {
        let tracks = vec![AudioTrack {
            index: 2,
            codec: None,
            channels: Some(2),
            language: None,
            title: None,
            is_default: true,
        }];
        assert!(validate_audio_track_index(2, &tracks).is_ok());
        assert!(validate_audio_track_index(0, &tracks).is_err());
        assert!(validate_audio_track_index(u32::MAX, &tracks).is_err());
        assert!(validate_client_event("ui.render.failed", &json!({ "count": 1 })).is_ok());
        assert!(validate_client_event("", &json!({})).is_err());
        assert!(validate_client_event("bad/event", &json!({})).is_err());
        assert!(validate_client_event(&"a".repeat(81), &json!({})).is_err());
        assert!(validate_client_event("valid", &json!([])).is_err());
        assert!(validate_client_event("valid", &json!({ "data": "x".repeat(16 * 1024) })).is_err());
    }

    #[test]
    fn generated_clips_are_rejected_by_raw_deletion_validation() {
        let clip = MediaItem {
            id: "a".repeat(64),
            kind: MediaKind::Clip,
            path: "C:\\clips\\protected.mp4".to_string(),
            source_path: Some("C:\\captures\\source.mp4".to_string()),
            filename: "protected.mp4".to_string(),
            file_size: 1,
            duration_seconds: 1.0,
            width: None,
            height: None,
            codec: None,
            tags: Vec::new(),
            notes: None,
            thumbnail_path: None,
            created_at: 1,
            modified_at: 1,
            trim_start_seconds: Some(0.0),
            trim_end_seconds: Some(1.0),
        };
        let error = validated_raw_deletion_path(&clip, &AppSettings::default())
            .expect_err("clip deletion must fail");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
}
