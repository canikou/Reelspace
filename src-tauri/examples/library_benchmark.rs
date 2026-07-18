use reelspace_lib::db::Database;
use reelspace_lib::ffmpeg::Ffmpeg;
use reelspace_lib::media::{normalized_path_key, scan_folder, ScanFolderResult};
use reelspace_lib::models::{MediaItem, MediaKind};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PassReport {
    elapsed_ms: u128,
    files_per_second: f64,
    files_visited: usize,
    indexed_files: usize,
    files_changed: usize,
    cache_hits: usize,
    files_reprobed: usize,
    files_skipped: usize,
    traversal_errors: usize,
    failures_by_code: HashMap<String, usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkReport {
    generated_at_unix_ms: u128,
    roots: Vec<String>,
    source_files: usize,
    source_bytes: u64,
    scratch_directory: String,
    cold: PassReport,
    warm: PassReport,
    warm_run_ms: Vec<u128>,
    warm_median_ms: u128,
    warm_p95_ms: u128,
    warm_speedup: f64,
    database_storage_bytes: u64,
    build_profile: &'static str,
    logical_processors: usize,
}

struct ScratchCleanup(PathBuf);

impl Drop for ScratchCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let roots = parse_roots()?;
    let (source_files, source_bytes) = inventory(&roots)?;
    let scratch = benchmark_scratch()?;
    fs::create_dir_all(scratch.join("thumbnails"))?;
    let _scratch_cleanup = ScratchCleanup(scratch.clone());

    for root in &roots {
        let canonical = root.canonicalize()?;
        if scratch.starts_with(&canonical) {
            return Err("benchmark scratch directory cannot be inside a media source".into());
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tool_dir = manifest
        .join("..")
        .join("vendor")
        .join("ffmpeg")
        .join("bin");
    let ffmpeg = Ffmpeg::from_paths(tool_dir.join("ffmpeg.exe"), tool_dir.join("ffprobe.exe"))?;
    let database_path = scratch.join("benchmark.sqlite");
    let database = Database::open(database_path.clone())?;

    let cold = run_pass(
        &roots,
        &ffmpeg,
        &database,
        &scratch.join("thumbnails"),
        &HashMap::new(),
    )?;
    let items = database.list_media(Some(MediaKind::Raw))?;
    let existing = items
        .into_iter()
        .map(|item| (normalized_path_key(Path::new(&item.path)), item))
        .collect::<HashMap<_, _>>();
    let mut warm_runs = Vec::new();
    for _ in 0..5 {
        warm_runs.push(run_pass(
            &roots,
            &ffmpeg,
            &database,
            &scratch.join("thumbnails"),
            &existing,
        )?);
    }
    let mut warm_run_ms = warm_runs
        .iter()
        .map(|pass| pass.elapsed_ms)
        .collect::<Vec<_>>();
    warm_run_ms.sort_unstable();
    let warm_median_ms = warm_run_ms[warm_run_ms.len() / 2];
    let warm_p95_ms = *warm_run_ms.last().expect("five warm runs");
    let warm_speedup = cold.elapsed_ms.max(1) as f64 / warm_median_ms.max(1) as f64;
    let warm = warm_runs.remove(warm_runs.len() / 2);
    let report = BenchmarkReport {
        generated_at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        roots: roots
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        source_files,
        source_bytes,
        scratch_directory: "isolated-system-temp".to_string(),
        cold,
        warm,
        warm_run_ms,
        warm_median_ms,
        warm_p95_ms,
        warm_speedup,
        database_storage_bytes: sqlite_storage_bytes(&database_path),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        logical_processors: std::thread::available_parallelism()?.get(),
    };
    let json = serde_json::to_string_pretty(&report)?;
    fs::write(scratch.join("report.json"), &json)?;
    if let Some(report_path) = env::var_os("REELSPACE_BENCHMARK_REPORT").map(PathBuf::from) {
        if let Some(parent) = report_path.parent() {
            fs::create_dir_all(parent)?;
        }
        for root in &roots {
            if report_path.starts_with(root.canonicalize()?) {
                return Err("benchmark report cannot be written inside a media source".into());
            }
        }
        fs::write(report_path, &json)?;
    }
    println!("{json}");
    Ok(())
}

fn sqlite_storage_bytes(path: &Path) -> u64 {
    [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ]
    .iter()
    .filter_map(|file| file.metadata().ok())
    .map(|metadata| metadata.len())
    .sum()
}

fn parse_roots() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let roots = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err(
            "usage: cargo run --example library_benchmark -- <media-root> [media-root...]".into(),
        );
    }
    for root in &roots {
        if !root.is_absolute() || !root.is_dir() {
            return Err(format!(
                "media root is not an available absolute directory: {}",
                root.display()
            )
            .into());
        }
    }
    Ok(roots)
}

fn benchmark_scratch() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(env::temp_dir().join(format!(
        "reelspace-library-benchmark-{}-{nonce}",
        std::process::id()
    )))
}

fn inventory(roots: &[PathBuf]) -> Result<(usize, u64), Box<dyn std::error::Error>> {
    let mut files = 0;
    let mut bytes = 0_u64;
    for root in roots {
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() && is_video(entry.path()) {
                files += 1;
                bytes = bytes.saturating_add(entry.metadata()?.len());
            }
        }
    }
    Ok((files, bytes))
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            ["mp4", "mkv", "mov", "avi", "webm", "m4v"]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

fn run_pass(
    roots: &[PathBuf],
    ffmpeg: &Ffmpeg,
    database: &Database,
    thumbnail_dir: &Path,
    existing: &HashMap<String, MediaItem>,
) -> Result<PassReport, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut aggregate = PassReport {
        elapsed_ms: 0,
        files_per_second: 0.0,
        files_visited: 0,
        indexed_files: 0,
        files_changed: 0,
        cache_hits: 0,
        files_reprobed: 0,
        files_skipped: 0,
        traversal_errors: 0,
        failures_by_code: HashMap::new(),
    };
    for root in roots {
        let result = scan_folder(root, ffmpeg, database, thumbnail_dir, existing, &[])?;
        persist(database, &result)?;
        aggregate.files_visited += result.files_visited;
        aggregate.indexed_files += result.items.len();
        aggregate.files_changed += result.files_changed;
        aggregate.cache_hits += result.cached_probe_hits;
        aggregate.files_reprobed += result.files_reprobed;
        aggregate.files_skipped += result.files_skipped;
        aggregate.traversal_errors += result.traversal_errors;
        for (code, count) in &result.failures_by_code {
            *aggregate.failures_by_code.entry(code.clone()).or_default() += count;
        }
    }
    aggregate.elapsed_ms = started.elapsed().as_millis();
    aggregate.files_per_second =
        aggregate.indexed_files as f64 / (aggregate.elapsed_ms.max(1) as f64 / 1000.0);
    Ok(aggregate)
}

fn persist(
    database: &Database,
    result: &ScanFolderResult,
) -> Result<(), Box<dyn std::error::Error>> {
    database.upsert_media_batch(&result.items)?;
    Ok(())
}
