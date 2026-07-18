use crate::db::Database;
use crate::error::AppResult;
use crate::ffmpeg::Ffmpeg;
use crate::jobs::{MediaJobQueue, PreviewCoordinator, ThumbnailScheduler};
use crate::models::AppSettings;
use crate::recovery::RecoveryJournal;
use crate::telemetry::Telemetry;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub ffmpeg: Ffmpeg,
    pub data_dir: PathBuf,
    pub thumbnail_dir: PathBuf,
    pub audio_preview_dir: PathBuf,
    pub media_index_lock: Arc<Mutex<()>>,
    pub scan_in_flight: Arc<AtomicBool>,
    pub scan_coalesced_events: Arc<AtomicU64>,
    pub audio_cache_cleanup_lock: Arc<Mutex<()>>,
    pub background_jobs: Arc<MediaJobQueue>,
    pub media_jobs: Arc<MediaJobQueue>,
    pub export_jobs: Arc<MediaJobQueue>,
    pub preview_coordinator: Arc<PreviewCoordinator>,
    pub thumbnail_scheduler: Arc<ThumbnailScheduler>,
    pub telemetry: Arc<Telemetry>,
    pub recovery: Arc<RecoveryJournal>,
    pub asset_scope: tauri::scope::fs::Scope,
}

impl AppState {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        let data_dir = match app.path().app_data_dir() {
            Ok(path) => path,
            Err(_) => fallback_data_dir()?,
        };
        let thumbnail_dir = data_dir.join("thumbnails");
        let audio_preview_dir = data_dir.join("audio-previews");
        fs::create_dir_all(&thumbnail_dir)?;
        fs::create_dir_all(&audio_preview_dir)?;
        let telemetry = Arc::new(Telemetry::new(&data_dir.join("logs"))?);
        let recovery = Arc::new(RecoveryJournal::begin(&data_dir)?);
        let (database, database_recovery) =
            Database::open_resilient(data_dir.join("reelspace.db"))?;
        let db = Arc::new(database);
        let asset_scope = app.asset_protocol_scope();
        authorize_asset_scope(
            &asset_scope,
            &db.load_settings()?,
            &data_dir,
            &thumbnail_dir,
            &audio_preview_dir,
        )?;
        if let Some(recovered) = database_recovery {
            telemetry.event(
                "error",
                "database.self_healed",
                serde_json::json!({
                    "reason": recovered.original_error,
                    "backupPath": recovered.backup_path,
                }),
            );
        }
        if recovery.recovered_unclean_session {
            telemetry.event(
                "warn",
                "app.unclean_shutdown_recovered",
                serde_json::json!({ "action": "resumed_persistent_jobs" }),
            );
        }
        let ffmpeg = Ffmpeg::discover(app);
        Ok(Self {
            db,
            ffmpeg,
            data_dir,
            thumbnail_dir,
            audio_preview_dir,
            media_index_lock: Arc::new(Mutex::new(())),
            scan_in_flight: Arc::new(AtomicBool::new(false)),
            scan_coalesced_events: Arc::new(AtomicU64::new(0)),
            audio_cache_cleanup_lock: Arc::new(Mutex::new(())),
            background_jobs: Arc::new(MediaJobQueue::new(1)),
            media_jobs: Arc::new(MediaJobQueue::new(1)),
            export_jobs: Arc::new(MediaJobQueue::new(1)),
            preview_coordinator: Arc::new(PreviewCoordinator::default()),
            thumbnail_scheduler: Arc::new(ThumbnailScheduler::default()),
            telemetry,
            recovery,
            asset_scope,
        })
    }

    pub fn authorize_settings_assets(&self, settings: &AppSettings) -> AppResult<()> {
        authorize_asset_scope(
            &self.asset_scope,
            settings,
            &self.data_dir,
            &self.thumbnail_dir,
            &self.audio_preview_dir,
        )
    }
}

fn authorize_asset_scope(
    scope: &tauri::scope::fs::Scope,
    settings: &AppSettings,
    data_dir: &std::path::Path,
    thumbnail_dir: &std::path::Path,
    audio_preview_dir: &std::path::Path,
) -> AppResult<()> {
    let managed_directories = [
        thumbnail_dir.to_path_buf(),
        audio_preview_dir.to_path_buf(),
        data_dir.join("clips"),
    ];
    for directory in managed_directories {
        scope.allow_directory(directory, true)?;
    }
    for directory in settings
        .source_folders
        .iter()
        .map(PathBuf::from)
        .chain(settings.working_folder.as_ref().map(PathBuf::from))
        .chain(settings.clip_folder.as_ref().map(PathBuf::from))
    {
        scope.allow_directory(directory, true)?;
    }
    Ok(())
}

fn fallback_data_dir() -> Result<PathBuf, std::io::Error> {
    if let Some(project_dirs) = ProjectDirs::from("com", "Reelspace", "Reelspace") {
        return Ok(project_dirs.data_dir().to_path_buf());
    }
    std::env::current_dir().map(|path| path.join(".reelspace"))
}
