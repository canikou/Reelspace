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
            let threshold =
                (limit as f64 * settings.cleanup_threshold_percent as f64 / 100.0) as u64;
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
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Component;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let root = path.components().find_map(|component| match component {
        Component::Prefix(prefix) => Some(format!("{}\\", prefix.as_os_str().to_string_lossy())),
        _ => None,
    })?;
    let mut wide = OsStr::new(&root).encode_wide().collect::<Vec<u16>>();
    wide.push(0);
    let mut available = 0u64;
    let mut total = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some((total, available))
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // statvfs field widths differ between targets
fn drive_space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    // statvfs on a missing folder fails; walk up to the nearest existing ancestor.
    let existing = path.ancestors().find(|candidate| candidate.exists())?;
    let c_path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    let fragment = stats.f_frsize as u64;
    Some((
        (stats.f_blocks as u64).saturating_mul(fragment),
        (stats.f_bavail as u64).saturating_mul(fragment),
    ))
}

#[cfg(not(any(windows, unix)))]
fn drive_space(_path: &Path) -> Option<(u64, u64)> {
    None
}
