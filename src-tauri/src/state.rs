use crate::db::Database;
use crate::error::AppResult;
use crate::ffmpeg::Ffmpeg;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub db: Database,
    pub ffmpeg: Ffmpeg,
    pub data_dir: PathBuf,
    pub thumbnail_dir: PathBuf,
}

impl AppState {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        let data_dir = app
            .path()
            .app_data_dir()
            .or_else(|_| fallback_data_dir())
            .map_err(|err| anyhow::anyhow!("could not resolve app data directory: {err}"))?;
        let thumbnail_dir = data_dir.join("thumbnails");
        fs::create_dir_all(&thumbnail_dir)?;
        let db = Database::open(data_dir.join("reelspace.db"))?;
        let ffmpeg = Ffmpeg::discover(app);
        Ok(Self {
            db,
            ffmpeg,
            data_dir,
            thumbnail_dir,
        })
    }
}

fn fallback_data_dir() -> Result<PathBuf, std::io::Error> {
    if let Some(project_dirs) = ProjectDirs::from("com", "Reelspace", "Reelspace") {
        return Ok(project_dirs.data_dir().to_path_buf());
    }
    std::env::current_dir().map(|path| path.join(".reelspace"))
}
