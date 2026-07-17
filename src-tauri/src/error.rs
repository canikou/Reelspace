use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    InvalidRequest,
    InvalidPath,
    PathOutsideManagedRoots,
    SourceUnavailable,
    PermissionDenied,
    FileNotFound,
    FileChanged,
    UnsupportedMedia,
    CorruptMedia,
    FfprobeUnavailable,
    FfprobeFailed,
    FfmpegUnavailable,
    FfmpegFailed,
    MediaProcessTimeout,
    MediaProcessCancelled,
    DatabaseUnavailable,
    DatabaseBusy,
    DatabaseMigrationFailed,
    DatabaseIntegrityFailed,
    DiskFull,
    DestinationCollision,
    ExportCancelled,
    ExportFailed,
    ThumbnailGenerationFailed,
    CacheOperationFailed,
    WindowsIntegrationFailed,
    Internal,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip)]
    detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: default_retryable(code),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn cancelled(operation: &str) -> Self {
        Self::new(
            ErrorCode::MediaProcessCancelled,
            format!("{operation} was cancelled"),
        )
    }

    pub fn timeout(operation: &str) -> Self {
        Self::new(
            ErrorCode::MediaProcessTimeout,
            format!("{operation} timed out"),
        )
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        let (code, message) = match error.kind() {
            std::io::ErrorKind::NotFound => (ErrorCode::FileNotFound, "The file was not found"),
            std::io::ErrorKind::PermissionDenied => (
                ErrorCode::PermissionDenied,
                "Reelspace does not have permission to access this file",
            ),
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                (ErrorCode::InvalidPath, "The file path is invalid")
            }
            _ if error.raw_os_error() == Some(112) => {
                (ErrorCode::DiskFull, "There is not enough free disk space")
            }
            _ => (ErrorCode::Internal, "A local file operation failed"),
        };
        Self::new(code, message).with_detail(error.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        let busy = matches!(
            &error,
            rusqlite::Error::SqliteFailure(failure, _)
                if matches!(
                    failure.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        );
        let (code, message) = if busy {
            (
                ErrorCode::DatabaseBusy,
                "The media library is busy; try again in a moment",
            )
        } else {
            (
                ErrorCode::DatabaseUnavailable,
                "The local media library could not be updated",
            )
        };
        Self::new(code, message).with_detail(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(
            ErrorCode::DatabaseIntegrityFailed,
            "Stored Reelspace data is invalid",
        )
        .with_detail(error.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(error: tauri::Error) -> Self {
        Self::new(ErrorCode::Internal, "The desktop operation failed")
            .with_detail(error.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(
            ErrorCode::Internal,
            "An internal Reelspace operation failed",
        )
        .with_detail(error.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn app_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message)
}

pub fn user_error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidRequest, message)
}

fn default_retryable(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::SourceUnavailable
            | ErrorCode::FileChanged
            | ErrorCode::DatabaseBusy
            | ErrorCode::DiskFull
            | ErrorCode::MediaProcessTimeout
            | ErrorCode::FfprobeFailed
            | ErrorCode::FfmpegFailed
            | ErrorCode::ThumbnailGenerationFailed
            | ErrorCode::CacheOperationFailed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialize_with_stable_machine_readable_fields() {
        let error = AppError::new(ErrorCode::SourceUnavailable, "Source is unavailable")
            .with_detail("C:\\private\\capture");
        let value = serde_json::to_value(error).expect("serialize error");
        assert_eq!(value["code"], "sourceUnavailable");
        assert_eq!(value["message"], "Source is unavailable");
        assert_eq!(value["retryable"], true);
        assert!(value.get("detail").is_none());
        assert!(!value.to_string().contains("private"));
    }

    #[test]
    fn cancellation_is_not_reported_as_an_unexpected_failure() {
        let error = AppError::cancelled("Clip export");
        assert_eq!(error.code, ErrorCode::MediaProcessCancelled);
        assert!(!error.retryable);
    }
}
