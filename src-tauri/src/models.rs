use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub source_folders: Vec<String>,
    pub working_folder: Option<String>,
    pub clip_folder: Option<String>,
    pub storage_limit_bytes: Option<u64>,
    pub cleanup_threshold_percent: u8,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            source_folders: Vec::new(),
            working_folder: None,
            clip_folder: None,
            storage_limit_bytes: None,
            cleanup_threshold_percent: 90,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Raw,
    Clip,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Clip => "clip",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "clip" => Self::Clip,
            _ => Self::Raw,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub kind: MediaKind,
    pub path: String,
    pub source_path: Option<String>,
    pub filename: String,
    pub file_size: u64,
    pub duration_seconds: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub thumbnail_path: Option<String>,
    pub created_at: i64,
    pub modified_at: i64,
    pub trim_start_seconds: Option<f64>,
    pub trim_end_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub duration_seconds: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
}

impl Default for MediaMetadata {
    fn default() -> Self {
        Self {
            duration_seconds: 0.0,
            width: None,
            height: None,
            codec: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub item_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClipRequest {
    pub source_id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub title: Option<String>,
    pub accurate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub drive_total_bytes: u64,
    pub drive_available_bytes: u64,
    pub raw_bytes: u64,
    pub clip_bytes: u64,
    pub managed_bytes: u64,
    pub storage_limit_bytes: Option<u64>,
    pub is_near_limit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub ffmpeg: ToolHealth,
    pub stats: StorageStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHealth {
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub ready: bool,
    pub message: String,
}
