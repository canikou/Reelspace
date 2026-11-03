//! Archive file safety: what may be pruned, retention, disk floor, crash-recovery naming.
//! Everything that deletes goes through the name matchers here — nothing else is ever touched.
use super::config::Retention;
use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: SystemTime,
}

fn digits(b: &[u8]) -> bool {
    b.iter().all(u8::is_ascii_digit)
}

/// `YYYY-MM-DD_HH-MM-SS`
pub fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 19
        && digits(&b[0..4])
        && b[4] == b'-'
        && digits(&b[5..7])
        && b[7] == b'-'
        && digits(&b[8..10])
        && b[10] == b'_'
        && digits(&b[11..13])
        && b[13] == b'-'
        && digits(&b[14..16])
        && b[16] == b'-'
        && digits(&b[17..19])
}

/// `^rec_\d{4}-\d\d-\d\d_\d\d-\d\d-\d\d\.mkv$`
pub fn is_rec_name(name: &str) -> bool {
    name.strip_prefix("rec_").and_then(|n| n.strip_suffix(".mkv")).is_some_and(is_timestamp)
}

/// `session_<slug>_<timestamp>.mkv`
pub fn is_session_name(name: &str) -> bool {
    name.strip_prefix("session_")
        .and_then(|n| n.strip_suffix(".mkv"))
        .is_some_and(|n| n.len() > 20 && n.as_bytes()[n.len() - 20] == b'_' && is_timestamp(&n[n.len() - 19..]))
}

/// `Video_<timestamp>.mkv` -> timestamp
pub fn video_timestamp(name: &str) -> Option<&str> {
    name.strip_prefix("Video_").and_then(|n| n.strip_suffix(".mkv")).filter(|t| is_timestamp(t))
}

/// Non-recursive listing of regular files (symlinks skipped).
pub fn list_files(dir: &Path) -> Vec<FileInfo> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.filter_map(|e| {
        let e = e.ok()?;
        let md = std::fs::symlink_metadata(e.path()).ok()?;
        md.is_file().then(|| FileInfo { path: e.path(), size: md.len(), mtime: md.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
    })
    .collect()
}

fn name_of(f: &FileInfo) -> &str {
    f.path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

pub fn rec_total(files: &[FileInfo]) -> u64 {
    files.iter().filter(|f| is_rec_name(name_of(f))).map(|f| f.size).sum()
}

fn oldest_first<'a>(files: &'a [FileInfo], pred: impl Fn(&str) -> bool) -> Vec<&'a FileInfo> {
    let mut v: Vec<_> = files.iter().filter(|f| pred(name_of(f))).collect();
    v.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.path.cmp(&b.path)));
    v
}

/// Oldest `rec_*` files to delete so the archive fits `cap_bytes` minus `headroom` (room for the next chunk).
pub fn select_cap_prune(files: &[FileInfo], cap_bytes: u64, headroom: u64) -> Vec<PathBuf> {
    let target = cap_bytes.saturating_sub(headroom);
    let mut total = rec_total(files);
    let mut out = vec![];
    for f in oldest_first(files, is_rec_name) {
        if total <= target {
            break;
        }
        total -= f.size;
        out.push(f.path.clone());
    }
    out
}

/// Oldest `rec_*` files (then expired sessions) to free at least `deficit` bytes. May return less if not enough exists.
pub fn select_space_prune(files: &[FileInfo], deficit: u64, retention: &Retention, now: SystemTime) -> Vec<PathBuf> {
    let mut freed = 0;
    let mut out = vec![];
    let expired = expired_sessions(files, retention, now);
    let recs = oldest_first(files, is_rec_name).into_iter().map(|f| (f.path.clone(), f.size));
    let sess = files.iter().filter(|f| expired.contains(&f.path)).map(|f| (f.path.clone(), f.size));
    for (p, s) in recs.chain(sess) {
        if freed >= deficit {
            break;
        }
        freed += s;
        out.push(p);
    }
    out
}

/// `session_*.mkv` files past retention. `KeepForever` never expires anything.
pub fn expired_sessions(files: &[FileInfo], retention: &Retention, now: SystemTime) -> Vec<PathBuf> {
    match retention {
        Retention::KeepForever => vec![],
        Retention::Days { days } => {
            let max = Duration::from_secs(*days as u64 * 86400);
            files
                .iter()
                .filter(|f| is_session_name(name_of(f)) && now.duration_since(f.mtime).is_ok_and(|age| age > max))
                .map(|f| f.path.clone())
                .collect()
        }
        Retention::SizeCapGb { gb } => {
            let cap = gb * super::config::GB;
            let mut total: u64 = files.iter().filter(|f| is_session_name(name_of(f))).map(|f| f.size).sum();
            let mut out = vec![];
            for f in oldest_first(files, is_session_name) {
                if total <= cap {
                    break;
                }
                total -= f.size;
                out.push(f.path.clone());
            }
            out
        }
    }
}

/// Bytes that must be freed before a new chunk may start (0 = fine). `next_chunk` is the estimated size of that chunk.
pub fn space_deficit(free: u64, floor: u64, next_chunk: u64) -> u64 {
    (floor + next_chunk).saturating_sub(free)
}

/// Delete exactly the given paths, re-checking each name against the safety matchers. Returns bytes freed.
pub fn delete_checked(paths: &[PathBuf]) -> u64 {
    let mut freed = 0;
    for p in paths {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(is_rec_name(name) || is_session_name(name)) {
            continue;
        }
        let size = std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(p).is_ok() {
            freed += size;
        }
    }
    freed
}

/// Delete every `rec_*.mkv` in `dir`; nothing else.
pub fn delete_rolling_archive(dir: &Path) -> u64 {
    let files = list_files(dir);
    let paths: Vec<_> = files.iter().filter(|f| is_rec_name(name_of(f))).map(|f| f.path.clone()).collect();
    delete_checked(&paths)
}

fn fold(base: char, orig: char) -> char {
    if orig.is_uppercase() { base.to_ascii_uppercase() } else { base }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = match c {
            'à'..='å' | 'À'..='Å' => fold('a', c),
            'ç' | 'Ç' => fold('c', c),
            'è'..='ë' | 'È'..='Ë' => fold('e', c),
            'ì'..='ï' | 'Ì'..='Ï' => fold('i', c),
            'ñ' | 'Ñ' => fold('n', c),
            'ò'..='ö' | 'Ò'..='Ö' | 'ø' | 'Ø' => fold('o', c),
            'ù'..='ü' | 'Ù'..='Ü' => fold('u', c),
            'ý' | 'ÿ' | 'Ý' => fold('y', c),
            c if c.is_ascii_alphanumeric() => c,
            c if c.is_whitespace() || c == '-' || c == '_' => '_',
            _ => continue, // ™ ® : / \ and other punctuation
        };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "game".into() } else { out }
}

pub fn session_file_name(game: &str, ts: &str) -> String {
    format!("session_{}_{ts}.mkv", slugify(game))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ActiveRecording {
    pub mode: String,
    pub game: Option<String>,
    pub appid: Option<u32>,
    /// RFC 3339, local offset.
    pub started_at: String,
    pub dir: PathBuf,
}

pub fn write_active(path: &Path, a: &ActiveRecording) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(a)?)?;
    std::fs::rename(tmp, path)
}

pub fn read_active(path: &Path) -> Option<ActiveRecording> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Final name for a leftover `Video_<ts>.mkv` found at startup.
pub fn recovered_name(video_name: &str, active: Option<&ActiveRecording>, rec_dir: &Path) -> Option<String> {
    let ts = video_timestamp(video_name)?;
    if let Some(a) = active.filter(|a| a.mode == "session" && a.dir == rec_dir) {
        let started = DateTime::parse_from_rfc3339(&a.started_at).ok()?.naive_local();
        let file_ts = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d_%H-%M-%S").ok()?;
        if file_ts >= started - chrono::Duration::seconds(10) {
            return Some(session_file_name(a.game.as_deref().unwrap_or("game"), ts));
        }
    }
    Some(format!("rec_{ts}.mkv"))
}

/// Adopt leftover `Video_*.mkv` files. Returns (old, new) pairs actually renamed. Never overwrites.
pub fn recover_leftovers(rec_dir: &Path, active: Option<&ActiveRecording>) -> Vec<(PathBuf, PathBuf)> {
    let mut out = vec![];
    for f in list_files(rec_dir) {
        let Some(new) = recovered_name(name_of(&f), active, rec_dir) else { continue };
        let dest = rec_dir.join(&new);
        if !dest.exists() && std::fs::rename(&f.path, &dest).is_ok() {
            if new.starts_with("session_") {
                if let Some(g) = active.and_then(|a| a.game.as_deref()) {
                    let _ = xattr::set(&dest, "user.reelspace.game", g.as_bytes());
                }
            }
            out.push((f.path, dest));
        }
    }
    out
}

pub fn now_ts() -> String {
    Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

pub fn is_mountpoint(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(a), Some(parent)) = (std::fs::metadata(path), path.parent()) else { return false };
    std::fs::metadata(parent).is_ok_and(|b| a.dev() != b.dev())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(name: &str, size: u64, age_days: u64) -> FileInfo {
        FileInfo {
            path: PathBuf::from("/r").join(name),
            size,
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(100_000_000 - age_days * 86400),
        }
    }
    const NOW_AGE0: SystemTime = SystemTime::UNIX_EPOCH;
    fn now() -> SystemTime {
        NOW_AGE0 + Duration::from_secs(100_000_000)
    }

    #[test]
    fn name_matchers() {
        assert!(is_rec_name("rec_2026-01-02_03-04-05.mkv"));
        for bad in ["rec_2026-01-02_03-04-05.mkv.part", "rec_x.mkv", "Rec_2026-01-02_03-04-05.mkv", "xrec_2026-01-02_03-04-05.mkv", "rec_2026-01-02_03-04-05.mp4", "clip_2026-01-02_03-04-05.mp4", "rec_2026-01-02_03-04-05.mkv\n"] {
            assert!(!is_rec_name(bad), "{bad}");
        }
        assert!(is_session_name("session_Elden_Ring_2026-01-02_03-04-05.mkv"));
        assert!(!is_session_name("session_2026-01-02_03-04-05.mkv"));
        assert!(!is_session_name("rec_2026-01-02_03-04-05.mkv"));
    }

    #[test]
    fn cap_prune_only_rec_oldest_first_with_headroom() {
        let files = vec![
            fi("rec_2026-01-01_00-00-00.mkv", 100, 5),
            fi("rec_2026-01-02_00-00-00.mkv", 100, 4),
            fi("rec_2026-01-03_00-00-00.mkv", 100, 3),
            fi("session_A_2026-01-01_00-00-00.mkv", 10_000, 9),
            fi("clip_2026-01-01_00-00-00.mp4", 10_000, 9),
            fi("notes.txt", 10_000, 9),
        ];
        // total rec=300, cap 300, headroom 150 -> target 150 -> delete oldest two
        let del = select_cap_prune(&files, 300, 150);
        assert_eq!(del, vec![PathBuf::from("/r/rec_2026-01-01_00-00-00.mkv"), PathBuf::from("/r/rec_2026-01-02_00-00-00.mkv")]);
        assert!(select_cap_prune(&files, 1000, 100).is_empty());
        // headroom larger than cap deletes all rec_ but nothing else
        let all = select_cap_prune(&files, 100, 500);
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|p| is_rec_name(p.file_name().unwrap().to_str().unwrap())));
    }

    #[test]
    fn retention_selection() {
        let files = vec![
            fi("session_A_2026-01-01_00-00-00.mkv", 100, 40),
            fi("session_B_2026-01-02_00-00-00.mkv", 100, 10),
            fi("rec_2026-01-02_00-00-00.mkv", 100, 400),
        ];
        assert!(expired_sessions(&files, &Retention::KeepForever, now()).is_empty());
        let d = expired_sessions(&files, &Retention::Days { days: 30 }, now());
        assert_eq!(d, vec![PathBuf::from("/r/session_A_2026-01-01_00-00-00.mkv")]);
        let big = vec![fi("session_A_2026-01-01_00-00-00.mkv", 2 * super::super::config::GB, 3), fi("session_B_2026-01-02_00-00-00.mkv", super::super::config::GB, 1)];
        let s = expired_sessions(&big, &Retention::SizeCapGb { gb: 2 }, now());
        assert_eq!(s, vec![PathBuf::from("/r/session_A_2026-01-01_00-00-00.mkv")]);
    }

    #[test]
    fn space_prune_prefers_rec_then_expired_sessions_never_keepforever() {
        let files = vec![
            fi("rec_2026-01-01_00-00-00.mkv", 100, 5),
            fi("session_A_2026-01-01_00-00-00.mkv", 1000, 99),
        ];
        assert_eq!(select_space_prune(&files, 50, &Retention::Days { days: 30 }, now()).len(), 1);
        assert_eq!(select_space_prune(&files, 500, &Retention::Days { days: 30 }, now()).len(), 2);
        assert_eq!(select_space_prune(&files, 500, &Retention::KeepForever, now()).len(), 1);
    }

    #[test]
    fn disk_floor_decision() {
        assert_eq!(space_deficit(100, 50, 20), 0);
        assert_eq!(space_deficit(60, 50, 20), 10);
        assert_eq!(space_deficit(0, 50, 20), 70);
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("The Witcher 3: Wild Hunt™"), "The_Witcher_3_Wild_Hunt");
        assert_eq!(slugify("Pokémon / Café\\Ñandú"), "Pokemon_CafeNandu");
        assert_eq!(slugify("™™"), "game");
        assert_eq!(slugify("  a   b  "), "a_b");
    }

    #[test]
    fn recovery_naming() {
        let dir = PathBuf::from("/r");
        let act = ActiveRecording {
            mode: "session".into(),
            game: Some("Hades II".into()),
            appid: Some(1),
            started_at: "2026-01-02T03:00:00+00:00".into(),
            dir: dir.clone(),
        };
        assert_eq!(recovered_name("Video_2026-01-02_03-00-02.mkv", Some(&act), &dir).unwrap(), "session_Hades_II_2026-01-02_03-00-02.mkv");
        // older than the recorded start -> belongs to the rolling archive
        assert_eq!(recovered_name("Video_2026-01-01_03-00-02.mkv", Some(&act), &dir).unwrap(), "rec_2026-01-01_03-00-02.mkv");
        assert_eq!(recovered_name("Video_2026-01-02_03-00-02.mkv", None, &dir).unwrap(), "rec_2026-01-02_03-00-02.mkv");
        let rolling = ActiveRecording { mode: "rolling".into(), ..act };
        assert_eq!(recovered_name("Video_2026-01-02_03-00-02.mkv", Some(&rolling), &dir).unwrap(), "rec_2026-01-02_03-00-02.mkv");
        assert!(recovered_name("random.mkv", None, &dir).is_none());
    }

    #[test]
    fn recover_and_delete_on_disk() {
        let d = tempfile::tempdir().unwrap();
        for n in ["Video_2026-01-02_03-00-02.mkv", "random.mkv", "rec_2026-01-01_00-00-00.mkv", "session_X_2026-01-01_00-00-00.mkv"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        std::fs::create_dir(d.path().join("Clips")).unwrap();
        std::fs::write(d.path().join("Clips/rec_2026-01-01_00-00-00.mkv"), b"keep").unwrap();
        let r = recover_leftovers(d.path(), None);
        assert_eq!(r.len(), 1);
        assert!(d.path().join("rec_2026-01-02_03-00-02.mkv").exists());
        assert!(d.path().join("random.mkv").exists());
        assert_eq!(delete_rolling_archive(d.path()), 2);
        assert!(d.path().join("session_X_2026-01-01_00-00-00.mkv").exists());
        assert!(d.path().join("random.mkv").exists());
        assert!(d.path().join("Clips/rec_2026-01-01_00-00-00.mkv").exists(), "never recursive");
    }

    #[test]
    fn active_state_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s/active.json");
        let a = ActiveRecording { mode: "rolling".into(), game: None, appid: None, started_at: "2026-01-02T03:00:00+00:00".into(), dir: "/r".into() };
        write_active(&p, &a).unwrap();
        assert_eq!(read_active(&p), Some(a));
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn free_and_mountpoint() {
        let d = tempfile::tempdir().unwrap();
        assert!(free_bytes(d.path()).unwrap() > 0);
        assert!(!is_mountpoint(&d.path().join("nonexistent")));
        assert!(is_mountpoint(Path::new("/proc")));
    }
}
