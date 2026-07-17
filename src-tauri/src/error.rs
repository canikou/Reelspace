use serde::Serialize;
use std::fmt;

#[allow(dead_code)]
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

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalidRequest",
            Self::InvalidPath => "invalidPath",
            Self::PathOutsideManagedRoots => "pathOutsideManagedRoots",
            Self::SourceUnavailable => "sourceUnavailable",
            Self::PermissionDenied => "permissionDenied",
            Self::FileNotFound => "fileNotFound",
            Self::FileChanged => "fileChanged",
            Self::UnsupportedMedia => "unsupportedMedia",
            Self::CorruptMedia => "corruptMedia",
            Self::FfprobeUnavailable => "ffprobeUnavailable",
            Self::FfprobeFailed => "ffprobeFailed",
            Self::FfmpegUnavailable => "ffmpegUnavailable",
            Self::FfmpegFailed => "ffmpegFailed",
            Self::MediaProcessTimeout => "mediaProcessTimeout",
            Self::MediaProcessCancelled => "mediaProcessCancelled",
            Self::DatabaseUnavailable => "databaseUnavailable",
            Self::DatabaseBusy => "databaseBusy",
            Self::DatabaseMigrationFailed => "databaseMigrationFailed",
            Self::DatabaseIntegrityFailed => "databaseIntegrityFailed",
            Self::DiskFull => "diskFull",
            Self::DestinationCollision => "destinationCollision",
            Self::ExportCancelled => "exportCancelled",
            Self::ExportFailed => "exportFailed",
            Self::ThumbnailGenerationFailed => "thumbnailGenerationFailed",
            Self::CacheOperationFailed => "cacheOperationFailed",
            Self::WindowsIntegrationFailed => "windowsIntegrationFailed",
            Self::Internal => "internal",
        }
    }
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
        let (code, message) = match &error {
            rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => (
                    ErrorCode::DatabaseBusy,
                    "The media library is busy; try again in a moment",
                ),
                rusqlite::ErrorCode::DiskFull => (
                    ErrorCode::DiskFull,
                    "There is not enough free disk space to update the media library",
                ),
                rusqlite::ErrorCode::PermissionDenied
                | rusqlite::ErrorCode::ReadOnly
                | rusqlite::ErrorCode::AuthorizationForStatementDenied => (
                    ErrorCode::PermissionDenied,
                    "Reelspace does not have permission to update its media library",
                ),
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => (
                    ErrorCode::DatabaseIntegrityFailed,
                    "The local media library is damaged and needs recovery",
                ),
                rusqlite::ErrorCode::ConstraintViolation
                | rusqlite::ErrorCode::TypeMismatch
                | rusqlite::ErrorCode::TooBig
                | rusqlite::ErrorCode::ParameterOutOfRange => (
                    ErrorCode::InvalidRequest,
                    "The requested library update contains invalid data",
                ),
                _ => (
                    ErrorCode::DatabaseUnavailable,
                    "The local media library could not be updated",
                ),
            },
            _ => (
                ErrorCode::DatabaseUnavailable,
                "The local media library could not be updated",
            ),
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

    #[test]
    fn every_error_code_has_a_unique_wire_name() {
        let codes = [
            ErrorCode::InvalidRequest,
            ErrorCode::InvalidPath,
            ErrorCode::PathOutsideManagedRoots,
            ErrorCode::SourceUnavailable,
            ErrorCode::PermissionDenied,
            ErrorCode::FileNotFound,
            ErrorCode::FileChanged,
            ErrorCode::UnsupportedMedia,
            ErrorCode::CorruptMedia,
            ErrorCode::FfprobeUnavailable,
            ErrorCode::FfprobeFailed,
            ErrorCode::FfmpegUnavailable,
            ErrorCode::FfmpegFailed,
            ErrorCode::MediaProcessTimeout,
            ErrorCode::MediaProcessCancelled,
            ErrorCode::DatabaseUnavailable,
            ErrorCode::DatabaseBusy,
            ErrorCode::DatabaseMigrationFailed,
            ErrorCode::DatabaseIntegrityFailed,
            ErrorCode::DiskFull,
            ErrorCode::DestinationCollision,
            ErrorCode::ExportCancelled,
            ErrorCode::ExportFailed,
            ErrorCode::ThumbnailGenerationFailed,
            ErrorCode::CacheOperationFailed,
            ErrorCode::WindowsIntegrationFailed,
            ErrorCode::Internal,
        ];
        let names = codes
            .iter()
            .map(|code| code.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), codes.len());
        for code in codes {
            let serialized = serde_json::to_value(AppError::new(code, "safe")).expect("serialize");
            assert_eq!(serialized["code"], code.as_str());
        }
    }

    #[test]
    fn io_failures_map_to_safe_specific_categories() {
        let cases = [
            (std::io::ErrorKind::NotFound, ErrorCode::FileNotFound),
            (
                std::io::ErrorKind::PermissionDenied,
                ErrorCode::PermissionDenied,
            ),
            (std::io::ErrorKind::InvalidInput, ErrorCode::InvalidPath),
            (std::io::ErrorKind::InvalidData, ErrorCode::InvalidPath),
            (std::io::ErrorKind::UnexpectedEof, ErrorCode::Internal),
        ];
        for (kind, expected) in cases {
            let error = AppError::from(std::io::Error::new(kind, "private-path"));
            assert_eq!(error.code, expected);
            assert!(!serde_json::to_string(&error)
                .expect("serialize")
                .contains("private-path"));
        }
        let disk_full = AppError::from(std::io::Error::from_raw_os_error(112));
        assert_eq!(disk_full.code, ErrorCode::DiskFull);
    }

    #[test]
    fn sqlite_failures_keep_recovery_categories_specific() {
        let cases = [
            (rusqlite::ffi::SQLITE_BUSY, ErrorCode::DatabaseBusy),
            (rusqlite::ffi::SQLITE_LOCKED, ErrorCode::DatabaseBusy),
            (rusqlite::ffi::SQLITE_FULL, ErrorCode::DiskFull),
            (rusqlite::ffi::SQLITE_PERM, ErrorCode::PermissionDenied),
            (rusqlite::ffi::SQLITE_READONLY, ErrorCode::PermissionDenied),
            (
                rusqlite::ffi::SQLITE_CORRUPT,
                ErrorCode::DatabaseIntegrityFailed,
            ),
            (
                rusqlite::ffi::SQLITE_NOTADB,
                ErrorCode::DatabaseIntegrityFailed,
            ),
            (rusqlite::ffi::SQLITE_CONSTRAINT, ErrorCode::InvalidRequest),
            (rusqlite::ffi::SQLITE_IOERR, ErrorCode::DatabaseUnavailable),
        ];
        for (sqlite_code, expected) in cases {
            let sqlite = rusqlite::ffi::Error::new(sqlite_code);
            let error = AppError::from(rusqlite::Error::SqliteFailure(sqlite, None));
            assert_eq!(error.code, expected);
        }
    }
}
