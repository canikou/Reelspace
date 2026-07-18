use reelspace_lib::db::Database;
use reelspace_lib::ffmpeg::Ffmpeg;
use reelspace_lib::media::{build_clip_item, ensure_thumbnail, scan_folder};
use reelspace_lib::models::{AppSettings, CreateClipRequest, MediaKind};
use reelspace_lib::portability::{export_library_data, import_library_data};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SmokeReport {
    indexed_raw_files: usize,
    malformed_files_isolated: usize,
    thumbnail_bytes: u64,
    clip_bytes: u64,
    trim_method: String,
    exported_items: u64,
    matched_items: u64,
    restored_items: u64,
    unmatched_items: u64,
    collections_created: u64,
    memberships_added: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (source_root, output_root, archive_path) = parse_args()?;
    if archive_path.exists() {
        return Err("the smoke archive destination already exists".into());
    }
    fs::create_dir_all(&output_root)?;
    let clip_path = output_root.join("reelspace-release-smoke-clip.mp4");
    if clip_path.exists() {
        return Err("the smoke clip destination already exists".into());
    }

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = env::temp_dir().join(format!(
        "reelspace-release-smoke-{}-{nonce}",
        std::process::id()
    ));
    let result = run_smoke(
        &source_root,
        &output_root,
        &archive_path,
        &clip_path,
        &scratch,
    );
    let _ = fs::remove_dir_all(&scratch);
    let report = result?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn run_smoke(
    source_root: &Path,
    output_root: &Path,
    archive_path: &Path,
    clip_path: &Path,
    scratch: &Path,
) -> Result<SmokeReport, Box<dyn std::error::Error>> {
    let source_root = source_root.canonicalize()?;
    let output_root = output_root.canonicalize()?;
    if output_root.starts_with(&source_root) || source_root.starts_with(&output_root) {
        return Err("smoke source and output roots must not overlap".into());
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tool_root = manifest
        .join("..")
        .join("vendor")
        .join("ffmpeg")
        .join("bin");
    let ffmpeg = Ffmpeg::from_paths(tool_root.join("ffmpeg.exe"), tool_root.join("ffprobe.exe"))?;
    let thumbnail_root = scratch.join("thumbnails");
    fs::create_dir_all(&thumbnail_root)?;
    let database = Database::open(scratch.join("source.sqlite"))?;

    let scan = scan_folder(
        &source_root,
        &ffmpeg,
        &database,
        &thumbnail_root,
        &HashMap::new(),
        std::slice::from_ref(&output_root),
    )?;
    database.upsert_media_batch(&scan.items)?;
    let raw = database.list_media(Some(MediaKind::Raw))?;
    if raw.len() != 1 {
        return Err(format!(
            "expected one valid sanitized recording; found {}",
            raw.len()
        )
        .into());
    }
    let malformed_files_isolated = scan.failures_by_code.values().sum::<usize>();
    if malformed_files_isolated == 0 {
        return Err("the malformed media fixture was not isolated".into());
    }

    let source = &raw[0];
    let thumbnail = ensure_thumbnail(
        &source.id,
        Path::new(&source.path),
        source.duration_seconds,
        &ffmpeg,
        &thumbnail_root,
        None,
    )?
    .ok_or("thumbnail generation returned no output")?;
    let thumbnail_bytes = thumbnail.metadata()?.len();
    if thumbnail_bytes == 0 {
        return Err("thumbnail output was empty".into());
    }

    let tags = vec!["release-smoke".to_string(), "portable".to_string()];
    database.update_tags(&source.id, &tags)?;
    database.update_notes(&source.id, Some("Sanitized release smoke note"))?;
    let collection_id = "9a42505edcb4b2ee66a3d4fef38d82549f9020a17ee0428f8bc29191b0b3e906";
    database.create_collection(collection_id, "Release Smoke", 1)?;
    database.add_to_collection(collection_id, &source.id, 1)?;
    let source = database.get_media(&source.id)?;

    let request = CreateClipRequest {
        source_id: source.id.clone(),
        session_id: Some("release-smoke".to_string()),
        start_seconds: 1.0,
        end_seconds: 3.0,
        title: Some("Sanitized release smoke clip".to_string()),
        enabled_audio_track_indices: Some(vec![1]),
    };
    let (clip, trim_method) = build_clip_item(
        &request,
        &source,
        clip_path.to_path_buf(),
        &ffmpeg,
        &thumbnail_root,
    )?;
    let clip_bytes = Path::new(&clip.path).metadata()?.len();
    if clip_bytes == 0 || clip.source_path.as_deref() != Some(source.path.as_str()) {
        return Err("generated clip metadata or output was invalid".into());
    }
    database.upsert_media(&clip)?;

    let settings = AppSettings {
        source_folders: vec![source_root.display().to_string()],
        working_folder: Some(output_root.display().to_string()),
        clip_folder: Some(output_root.display().to_string()),
        storage_limit_bytes: None,
        cleanup_threshold_percent: 90,
    };
    let exported = export_library_data(&database, &settings, scratch, archive_path)?;
    if exported.items != 2 || exported.collections != 1 {
        return Err("the metadata-only archive did not contain the expected records".into());
    }

    let import_thumbnail_root = scratch.join("import-thumbnails");
    fs::create_dir_all(&import_thumbnail_root)?;
    let imported_database = Database::open(scratch.join("import.sqlite"))?;
    let import_scan = scan_folder(
        &source_root,
        &ffmpeg,
        &imported_database,
        &import_thumbnail_root,
        &HashMap::new(),
        std::slice::from_ref(&output_root),
    )?;
    imported_database.upsert_media_batch(&import_scan.items)?;
    let imported = import_library_data(
        &imported_database,
        &settings,
        &scratch.join("import-data"),
        archive_path,
    )?;
    if imported.restored_items != 1 || imported.unmatched_items != 0 {
        return Err("the existing generated clip reference did not restore safely".into());
    }
    let imported_raw = imported_database.list_media(Some(MediaKind::Raw))?;
    if imported_raw.len() != 1 {
        return Err("portable import did not preserve one local recording".into());
    }
    let portable_tags_present = tags.iter().all(|tag| imported_raw[0].tags.contains(tag));
    if !portable_tags_present
        || imported_raw[0].notes.as_deref() != Some("Sanitized release smoke note")
    {
        return Err("portable metadata did not round-trip onto the local recording".into());
    }
    let collections = imported_database.list_collections()?;
    if collections.len() != 1 || collections[0].item_count != 1 {
        return Err("portable collection membership did not round-trip".into());
    }

    Ok(SmokeReport {
        indexed_raw_files: raw.len(),
        malformed_files_isolated,
        thumbnail_bytes,
        clip_bytes,
        trim_method: trim_method.as_str().to_string(),
        exported_items: exported.items,
        matched_items: imported.matched_items,
        restored_items: imported.restored_items,
        unmatched_items: imported.unmatched_items,
        collections_created: imported.collections_created,
        memberships_added: imported.memberships_added,
    })
}

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let args = env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: release_media_smoke <source-root> <output-root> <archive.json>".into());
    }
    if !args[0].is_absolute() || !args[0].is_dir() {
        return Err("the smoke source must be an available absolute directory".into());
    }
    if !args[1].is_absolute() || !args[2].is_absolute() {
        return Err("the smoke output and archive paths must be absolute".into());
    }
    Ok((args[0].clone(), args[1].clone(), args[2].clone()))
}
