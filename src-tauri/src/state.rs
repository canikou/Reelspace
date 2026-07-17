use crate::db::Database;
use crate::error::AppResult;
use crate::ffmpeg::Ffmpeg;
use crate::jobs::{MediaJobQueue, PreviewCoordinator, ThumbnailScheduler};
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
        let db = Arc::new(Database::open(data_dir.join("reelspace.db"))?);
        let ffmpeg = Ffmpeg::discover(app);
        let telemetry = Arc::new(Telemetry::new(&data_dir.join("logs"))?);
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
        })
    }
}

fn fallback_data_dir() -> Result<PathBuf, std::io::Error> {
    if let Some(project_dirs) = ProjectDirs::from("com", "Reelspace", "Reelspace") {
        return Ok(project_dirs.data_dir().to_path_buf());
    }
    std::env::current_dir().map(|path| path.join(".reelspace"))
}
