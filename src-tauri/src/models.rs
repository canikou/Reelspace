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
    pub theme: String,
    pub automatically_check_updates: bool,
    pub automatically_download_updates: bool,
    pub include_prerelease_updates: bool,
    pub preferred_recording_app: Option<String>,
    pub automatically_archive_recordings: bool,
    pub automatically_archive_clips: bool,
    pub keep_local_clips: bool,
    pub pause_uploads_while_recording: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            source_folders: Vec::new(),
            working_folder: None,
            clip_folder: None,
            storage_limit_bytes: None,
            cleanup_threshold_percent: 90,
            theme: "system".to_owned(),
            automatically_check_updates: true,
            automatically_download_updates: false,
            include_prerelease_updates: true,
            preferred_recording_app: None,
            automatically_archive_recordings: false,
            automatically_archive_clips: false,
            keep_local_clips: true,
            pause_uploads_while_recording: true,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub duration_seconds: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
    pub container: Option<String>,
    pub frame_rate: Option<f64>,
    pub rotation_degrees: Option<i32>,
    pub creation_timestamp: Option<String>,
}

impl Default for MediaMetadata {
    fn default() -> Self {
        Self {
            duration_seconds: 0.0,
            width: None,
            height: None,
            codec: None,
            container: None,
            frame_rate: None,
            rotation_degrees: None,
            creation_timestamp: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeData {
    pub metadata: MediaMetadata,
    pub audio_tracks: Vec<AudioTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub item_count: u32,
    pub storage_default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDataExportSummary {
    pub items: u64,
    pub collections: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDataImportSummary {
    pub matched_items: u64,
    pub restored_items: u64,
    pub unmatched_items: u64,
    pub updated_items: u64,
    pub tag_conflicts: u64,
    pub note_conflicts: u64,
    pub collections_created: u64,
    pub memberships_added: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClipRequest {
    pub source_id: String,
    pub session_id: Option<String>,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub title: Option<String>,
    #[serde(default)]
    pub enabled_audio_track_indices: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub index: u32,
    pub codec: Option<String>,
    pub channels: Option<u32>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPreviewResult {
    pub path: String,
    pub cache_hit: bool,
    pub method: String,
    pub elapsed_ms: u64,
    pub bytes: u64,
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
    pub version: String,
    pub settings: AppSettings,
    pub ffmpeg: ToolHealth,
    pub stats: StorageStats,
    pub active_media_work: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHealth {
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub ready: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub name: String,
    pub color: String,
    pub media_count: u64,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameAssignment {
    pub media_id: String,
    pub game_id: String,
    pub game_name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStatus {
    pub media_id: String,
    pub state: String,
    pub keep_local: bool,
    pub remote_copy_count: u64,
    pub primary_remote_url: Option<String>,
    pub progress_percent: u8,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveAccount {
    pub provider: String,
    pub account_id: String,
    pub display_name: String,
    pub connected_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadJob {
    pub id: String,
    pub media_id: String,
    pub filename: String,
    pub state: String,
    pub priority: i64,
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
    pub attempts: u32,
    pub error: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaEvent {
    pub event: String,
    pub detail: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecorderApp {
    pub id: String,
    pub name: String,
    pub executable_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepairSummary {
    pub missing_local_files: u64,
    pub preserved_remote_only_items: u64,
    pub thumbnails_queued: u64,
    pub stale_jobs_recovered: u64,
}
