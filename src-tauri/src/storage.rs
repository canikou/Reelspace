use crate::models::{AppSettings, MediaItem, MediaKind, StorageStats};
use std::path::Path;

pub fn stats(items: &[MediaItem], settings: &AppSettings) -> StorageStats {
    let raw_bytes = items
        .iter()
        .filter(|item| item.kind == MediaKind::Raw)
        .map(|item| item.file_size)
        .sum();
    let clip_bytes = items
        .iter()
        .filter(|item| item.kind == MediaKind::Clip)
        .map(|item| item.file_size)
        .sum();
    let managed_bytes = raw_bytes + clip_bytes;
    let (drive_total_bytes, drive_available_bytes) = storage_anchor(settings)
        .and_then(drive_space)
        .unwrap_or((0, 0));
    let is_near_limit = settings
        .storage_limit_bytes
        .map(|limit| {
            let threshold = (limit as f64 * settings.cleanup_threshold_percent as f64 / 100.0) as u64;
            managed_bytes >= threshold
        })
        .unwrap_or(false);

    StorageStats {
        drive_total_bytes,
        drive_available_bytes,
        raw_bytes,
        clip_bytes,
        managed_bytes,
        storage_limit_bytes: settings.storage_limit_bytes,
        is_near_limit,
    }
}

pub fn cleanup_candidates(items: &[MediaItem]) -> Vec<MediaItem> {
    let mut candidates = items
        .iter()
        .filter(|item| item.kind == MediaKind::Raw)
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|item| item.created_at);
    candidates
}

fn storage_anchor(settings: &AppSettings) -> Option<&Path> {
    settings
        .working_folder
        .as_deref()
        .or_else(|| settings.source_folders.first().map(String::as_str))
        .map(Path::new)
}

#[cfg(windows)]
fn drive_space(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    let root = path.components().next().map(|component| component.as_os_str())?;
    let mut wide = root.encode_wide().collect::<Vec<u16>>();
    wide.push('\\' as u16);
    wide.push(0);
    let mut available = 0u64;
    let mut total = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut available),
            Some(&mut total),
            None,
        )
    };
    ok.as_bool().then_some((total, available))
}

#[cfg(not(windows))]
fn drive_space(_path: &Path) -> Option<(u64, u64)> {
    None
}
