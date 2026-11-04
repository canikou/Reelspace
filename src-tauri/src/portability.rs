use crate::db::{
    Database, PortableCollectionUpdate, PortableGameUpdate, PortableImportBatch,
    PortableMediaFeatureUpdate, PortableMediaUpdate, PortableRemoteAssetUpdate,
};
use crate::error::{user_error, AppError, AppResult};
use crate::media::normalized_path_key;
use crate::models::{
    AppSettings, LibraryDataExportSummary, LibraryDataImportSummary, MediaItem, MediaKind,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const ARCHIVE_FORMAT: &str = "reelspace-library-data";
const ARCHIVE_VERSION: u32 = 2;
const MIN_ARCHIVE_VERSION: u32 = 1;
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 250_000;
const MAX_COLLECTIONS: usize = 50_000;
const MAX_MEMBERSHIPS: usize = 2_000_000;
const MAX_PATH_BYTES: usize = 32_767;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryArchive {
    format: String,
    version: u32,
    exported_at: i64,
    items: Vec<ArchiveItem>,
    collections: Vec<ArchiveCollection>,
    #[serde(default)]
    games: Vec<ArchiveGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveItem {
    archive_id: String,
    kind: MediaKind,
    original_path: String,
    relative_path: Option<String>,
    original_source_path: Option<String>,
    source_relative_path: Option<String>,
    filename: String,
    file_size: u64,
    duration_seconds: f64,
    width: Option<u32>,
    height: Option<u32>,
    codec: Option<String>,
    tags: Vec<String>,
    notes: Option<String>,
    created_at: i64,
    modified_at: i64,
    trim_start_seconds: Option<f64>,
    trim_end_seconds: Option<f64>,
    #[serde(default)]
    game_id: Option<String>,
    #[serde(default)]
    keep_local: Option<bool>,
    #[serde(default)]
    remote_assets: Vec<ArchiveRemoteAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveGame {
    archive_id: String,
    name: String,
    color: String,
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveRemoteAsset {
    provider: String,
    provider_remote_id: String,
    browser_url: Option<String>,
    privacy_status: Option<String>,
    verification_status: String,
    processing_status: String,
    verified_at: Option<i64>,
    last_checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveCollection {
    archive_id: String,
    name: String,
    member_ids: Vec<String>,
    #[serde(default)]
    storage_default: Option<String>,
}

fn portable_remote_assets(
    database: &Database,
) -> AppResult<HashMap<String, Vec<ArchiveRemoteAsset>>> {
    let conn = database.conn()?;
    let mut statement = conn.prepare(
        "SELECT media_id, provider, provider_remote_id, browser_url, privacy_status,
                verification_status, processing_status, verified_at, last_checked_at
         FROM remote_assets ORDER BY media_id, provider, provider_remote_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ArchiveRemoteAsset {
                provider: row.get(1)?,
                provider_remote_id: row.get(2)?,
                browser_url: row.get(3)?,
                privacy_status: row.get(4)?,
                verification_status: row.get(5)?,
                processing_status: row.get(6)?,
                verified_at: row.get(7)?,
                last_checked_at: row.get(8)?,
            },
        ))
    })?;
    let mut assets = HashMap::<String, Vec<ArchiveRemoteAsset>>::new();
    for row in rows {
        let (media_id, asset) = row?;
        assets.entry(media_id).or_default().push(asset);
    }
    Ok(assets)
}

pub fn export_library_data(
    database: &Database,
    settings: &AppSettings,
    data_dir: &Path,
    destination: &Path,
) -> AppResult<LibraryDataExportSummary> {
    validate_archive_path(destination, false)?;
    let roots = reference_roots(settings, data_dir);
    let media = database.list_media(None)?;
    if media.len() > MAX_ITEMS {
        return Err(user_error(
            "the library is too large for a portable data archive",
        ));
    }
    let collections = database.list_collections()?;
    if collections.len() > MAX_COLLECTIONS {
        return Err(user_error("there are too many collections to export"));
    }
    let games = database.list_games()?;
    let game_assignments = database
        .list_game_assignments()?
        .into_iter()
        .map(|assignment| (assignment.media_id, assignment.game_id))
        .collect::<HashMap<_, _>>();
    let keep_local = database
        .list_archive_statuses()?
        .into_iter()
        .map(|status| (status.media_id, status.keep_local))
        .collect::<HashMap<_, _>>();
    let remote_assets = portable_remote_assets(database)?;
    let items = media
        .iter()
        .map(|item| ArchiveItem {
            archive_id: item.id.clone(),
            kind: item.kind,
            original_path: item.path.clone(),
            relative_path: relative_to_roots(Path::new(&item.path), &roots),
            original_source_path: item.source_path.clone(),
            source_relative_path: item
                .source_path
                .as_deref()
                .and_then(|path| relative_to_roots(Path::new(path), &roots)),
            filename: item.filename.clone(),
            file_size: item.file_size,
            duration_seconds: item.duration_seconds,
            width: item.width,
            height: item.height,
            codec: item.codec.clone(),
            tags: item.tags.clone(),
            notes: item.notes.clone(),
            created_at: item.created_at,
            modified_at: item.modified_at,
            trim_start_seconds: item.trim_start_seconds,
            trim_end_seconds: item.trim_end_seconds,
            game_id: game_assignments.get(&item.id).cloned(),
            keep_local: keep_local.get(&item.id).copied(),
            remote_assets: remote_assets.get(&item.id).cloned().unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let mut membership_count = 0usize;
    let mut archive_collections = Vec::with_capacity(collections.len());
    for collection in collections {
        let member_ids = database
            .list_collection_media(&collection.id)?
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        membership_count = membership_count.saturating_add(member_ids.len());
        if membership_count > MAX_MEMBERSHIPS {
            return Err(user_error(
                "there are too many collection memberships to export",
            ));
        }
        archive_collections.push(ArchiveCollection {
            archive_id: collection.id,
            name: collection.name,
            member_ids,
            storage_default: collection.storage_default,
        });
    }
    let archive = LibraryArchive {
        format: ARCHIVE_FORMAT.to_string(),
        version: ARCHIVE_VERSION,
        exported_at: Utc::now().timestamp(),
        items,
        collections: archive_collections,
        games: games
            .into_iter()
            .map(|game| ArchiveGame {
                archive_id: game.id,
                name: game.name,
                color: game.color,
                aliases: game.aliases,
            })
            .collect(),
    };
    let mut writer = BoundedWriter::new(MAX_ARCHIVE_BYTES as usize);
    if let Err(error) = serde_json::to_writer_pretty(&mut writer, &archive) {
        if writer.exceeded {
            return Err(user_error(
                "the portable data archive exceeds the 64 MB safety limit",
            ));
        }
        return Err(invalid_archive(error));
    }
    let bytes = writer.bytes;
    atomic_write(destination, &bytes)?;
    Ok(LibraryDataExportSummary {
        items: archive.items.len() as u64,
        collections: archive.collections.len() as u64,
        bytes: bytes.len() as u64,
    })
}

pub fn import_library_data(
    database: &Database,
    settings: &AppSettings,
    data_dir: &Path,
    source: &Path,
) -> AppResult<LibraryDataImportSummary> {
    validate_archive_path(source, true)?;
    let metadata = fs::metadata(source)?;
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(user_error(
            "the portable data archive exceeds the 64 MB safety limit",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(source)?
        .take(MAX_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(user_error(
            "the portable data archive exceeds the 64 MB safety limit",
        ));
    }
    let archive: LibraryArchive = serde_json::from_slice(&bytes).map_err(invalid_archive)?;
    validate_archive(&archive)?;

    let roots = reference_roots(settings, data_dir);
    let existing_games = database.list_games()?;
    let mut game_ids = HashMap::<String, String>::new();
    let games = archive
        .games
        .iter()
        .map(|game| {
            let local_id = existing_games
                .iter()
                .find(|existing| existing.name.eq_ignore_ascii_case(&game.name))
                .map(|existing| existing.id.clone())
                .unwrap_or_else(|| stable_id(&format!("game:{}", game.name.trim().to_lowercase())));
            game_ids.insert(game.archive_id.clone(), local_id.clone());
            PortableGameUpdate {
                id: local_id,
                name: game.name.clone(),
                color: game.color.clone(),
                aliases: game.aliases.clone(),
            }
        })
        .collect::<Vec<_>>();
    let current = database.list_media(None)?;
    let by_path = current
        .iter()
        .enumerate()
        .map(|(index, item)| (normalized_path_key(Path::new(&item.path)), index))
        .collect::<HashMap<_, _>>();
    let mut matched_items = 0u64;
    let mut restored_items = Vec::new();
    let mut updates = Vec::new();
    let mut tag_conflicts = 0u64;
    let mut note_conflicts = 0u64;
    let mut imported_ids = HashMap::<String, String>::new();
    let mut claimed_local_ids = HashSet::new();

    for archived in &archive.items {
        if let Some(index) = match_existing(archived, &current, &by_path, &roots) {
            let local = &current[index];
            if !claimed_local_ids.insert(local.id.clone()) {
                continue;
            }
            matched_items += 1;
            imported_ids.insert(archived.archive_id.clone(), local.id.clone());
            let (tags, tags_truncated) = merge_tags(&local.tags, &archived.tags)?;
            tag_conflicts += u64::from(tags_truncated);
            let notes = match (&local.notes, &archived.notes) {
                (Some(existing), Some(imported)) if existing != imported => {
                    note_conflicts += 1;
                    Some(existing.clone())
                }
                (None, Some(imported)) => Some(imported.clone()),
                (existing, _) => existing.clone(),
            };
            if tags != local.tags || notes != local.notes {
                updates.push(PortableMediaUpdate {
                    media_id: local.id.clone(),
                    tags,
                    notes,
                });
            }
            continue;
        }
        let resolved_local_path = resolve_existing_media_path(archived, &roots);
        let path = resolved_local_path.clone().or_else(|| {
            archived
                .remote_assets
                .iter()
                .any(|asset| asset.verification_status == "verified")
                .then(|| remote_only_placeholder(data_dir, archived))
        });
        if let Some(path) = path {
            let path_string = path.to_string_lossy().to_string();
            let id = stable_id(&path_string);
            if by_path.contains_key(&normalized_path_key(&path))
                || !claimed_local_ids.insert(id.clone())
            {
                continue;
            }
            imported_ids.insert(archived.archive_id.clone(), id.clone());
            restored_items.push(MediaItem {
                id,
                kind: archived.kind,
                path: path_string,
                source_path: resolved_local_path
                    .as_ref()
                    .and_then(|_| resolve_source_path(archived, &roots)),
                filename: path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&archived.filename)
                    .to_string(),
                file_size: archived.file_size,
                duration_seconds: archived.duration_seconds,
                width: archived.width,
                height: archived.height,
                codec: archived.codec.clone(),
                tags: archived.tags.clone(),
                notes: archived.notes.clone(),
                thumbnail_path: None,
                created_at: archived.created_at,
                modified_at: archived.modified_at,
                trim_start_seconds: archived.trim_start_seconds,
                trim_end_seconds: archived.trim_end_seconds,
            });
        }
    }

    let existing_collections = database.list_collections()?;
    let existing_by_name = existing_collections
        .into_iter()
        .map(|collection| (collection.name.to_lowercase(), collection.id))
        .collect::<HashMap<_, _>>();
    let collections = archive
        .collections
        .iter()
        .map(|collection| PortableCollectionUpdate {
            id: existing_by_name
                .get(&collection.name.to_lowercase())
                .cloned()
                .unwrap_or_else(|| {
                    stable_id(&format!("collection:{}", collection.name.to_lowercase()))
                }),
            name: collection.name.clone(),
            media_ids: collection
                .member_ids
                .iter()
                .filter_map(|archive_id| imported_ids.get(archive_id).cloned())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            storage_default: collection.storage_default.clone(),
        })
        .collect::<Vec<_>>();
    let media_features = archive
        .items
        .iter()
        .filter_map(|item| {
            imported_ids
                .get(&item.archive_id)
                .map(|media_id| PortableMediaFeatureUpdate {
                    media_id: media_id.clone(),
                    game_id: item
                        .game_id
                        .as_ref()
                        .and_then(|archive_game_id| game_ids.get(archive_game_id))
                        .cloned(),
                    keep_local: item.keep_local,
                })
        })
        .collect::<Vec<_>>();
    let remote_assets = archive
        .items
        .iter()
        .filter_map(|item| {
            imported_ids
                .get(&item.archive_id)
                .map(|media_id| (media_id, &item.remote_assets))
        })
        .flat_map(|(media_id, assets)| {
            assets.iter().map(move |asset| PortableRemoteAssetUpdate {
                id: stable_id(&format!(
                    "remote:{}:{}",
                    asset.provider, asset.provider_remote_id
                )),
                media_id: media_id.clone(),
                provider: asset.provider.clone(),
                provider_remote_id: asset.provider_remote_id.clone(),
                browser_url: asset.browser_url.clone(),
                privacy_status: asset.privacy_status.clone(),
                verification_status: asset.verification_status.clone(),
                processing_status: asset.processing_status.clone(),
                verified_at: asset.verified_at,
                last_checked_at: asset.last_checked_at,
            })
        })
        .collect::<Vec<_>>();
    let changes = database.apply_portable_import(
        PortableImportBatch {
            restored_items: &restored_items,
            media_updates: &updates,
            collections: &collections,
            games: &games,
            media_features: &media_features,
            remote_assets: &remote_assets,
        },
        Utc::now().timestamp(),
    )?;
    Ok(LibraryDataImportSummary {
        matched_items,
        restored_items: restored_items.len() as u64,
        unmatched_items: archive.items.len() as u64 - matched_items - restored_items.len() as u64,
        updated_items: updates.len() as u64 + restored_items.len() as u64,
        tag_conflicts,
        note_conflicts,
        collections_created: changes.collections_created,
        memberships_added: changes.memberships_added,
    })
}

fn validate_archive(archive: &LibraryArchive) -> AppResult<()> {
    if archive.format != ARCHIVE_FORMAT {
        return Err(user_error(
            "the selected file is not a Reelspace data archive",
        ));
    }
    if !(MIN_ARCHIVE_VERSION..=ARCHIVE_VERSION).contains(&archive.version) {
        return Err(user_error(format!(
            "Reelspace data archive version {} is not supported",
            archive.version
        )));
    }
    if archive.items.len() > MAX_ITEMS {
        return Err(user_error(
            "the data archive contains too many media records",
        ));
    }
    if archive.collections.len() > MAX_COLLECTIONS {
        return Err(user_error("the data archive contains too many collections"));
    }
    if archive.games.len() > MAX_COLLECTIONS {
        return Err(user_error("the data archive contains too many games"));
    }
    let mut game_ids = HashSet::with_capacity(archive.games.len());
    let mut game_names = HashSet::with_capacity(archive.games.len());
    for game in &archive.games {
        validate_id(&game.archive_id, "game")?;
        if !game_ids.insert(game.archive_id.as_str()) {
            return Err(user_error(
                "the data archive contains duplicate game identifiers",
            ));
        }
        let name = game.name.trim();
        let normalized_name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty()
            || name != game.name
            || normalized_name != name
            || name.chars().count() > 120
            || contains_forbidden_control(name, false)
            || !game_names.insert(name.to_lowercase())
        {
            return Err(user_error("an imported game name is invalid or duplicated"));
        }
        if game.color.len() != 7
            || !game.color.starts_with('#')
            || !game.color[1..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(user_error("an imported game color is invalid"));
        }
        if game.aliases.len() > 1_000 {
            return Err(user_error("an imported game has too many aliases"));
        }
        let mut aliases = HashSet::with_capacity(game.aliases.len());
        for original_alias in &game.aliases {
            let alias = original_alias.trim();
            let normalized_alias = alias.split_whitespace().collect::<Vec<_>>().join(" ");
            if alias.is_empty()
                || alias != original_alias
                || normalized_alias != alias
                || alias.chars().count() > 120
                || contains_forbidden_control(alias, false)
                || !aliases.insert(alias.to_lowercase())
            {
                return Err(user_error(
                    "an imported game alias is invalid or duplicated",
                ));
            }
        }
    }
    let mut ids = HashSet::with_capacity(archive.items.len());
    let mut original_paths = HashSet::with_capacity(archive.items.len());
    let mut remote_ids = HashSet::new();
    for item in &archive.items {
        validate_id(&item.archive_id, "media")?;
        if !ids.insert(item.archive_id.as_str()) {
            return Err(user_error(
                "the data archive contains duplicate media identifiers",
            ));
        }
        if !original_paths.insert(normalized_path_key(Path::new(&item.original_path))) {
            return Err(user_error(
                "the data archive contains duplicate media paths",
            ));
        }
        validate_item(item)?;
        for asset in &item.remote_assets {
            if !remote_ids.insert((asset.provider.as_str(), asset.provider_remote_id.as_str())) {
                return Err(user_error(
                    "the data archive contains duplicate remote references",
                ));
            }
        }
        if item
            .game_id
            .as_ref()
            .is_some_and(|game_id| !game_ids.contains(game_id.as_str()))
        {
            return Err(user_error(
                "a media item references a game missing from the data archive",
            ));
        }
    }
    let mut collection_ids = HashSet::with_capacity(archive.collections.len());
    let mut collection_names = HashSet::with_capacity(archive.collections.len());
    let mut memberships = 0usize;
    for collection in &archive.collections {
        validate_id(&collection.archive_id, "collection")?;
        if !collection_ids.insert(collection.archive_id.as_str()) {
            return Err(user_error(
                "the data archive contains duplicate collection identifiers",
            ));
        }
        let name = validate_collection_name(&collection.name)?;
        if name != collection.name {
            return Err(user_error("an imported collection name is invalid"));
        }
        if !collection_names.insert(name.to_lowercase()) {
            return Err(user_error(
                "the data archive contains duplicate collection names",
            ));
        }
        if !matches!(
            collection.storage_default.as_deref(),
            None | Some("keepLocal") | Some("archive")
        ) {
            return Err(user_error(
                "an imported collection storage default is invalid",
            ));
        }
        memberships = memberships.saturating_add(collection.member_ids.len());
        if memberships > MAX_MEMBERSHIPS {
            return Err(user_error(
                "the data archive contains too many collection memberships",
            ));
        }
        let mut members = HashSet::with_capacity(collection.member_ids.len());
        for member in &collection.member_ids {
            validate_id(member, "media")?;
            if !ids.contains(member.as_str()) {
                return Err(user_error(
                    "a collection references media missing from the data archive",
                ));
            }
            if !members.insert(member.as_str()) {
                return Err(user_error(
                    "a collection contains duplicate media references",
                ));
            }
        }
    }
    Ok(())
}

fn validate_item(item: &ArchiveItem) -> AppResult<()> {
    validate_path_text(&item.original_path, "media path")?;
    if let Some(path) = &item.relative_path {
        validate_relative_path(path)?;
    }
    if let Some(path) = &item.original_source_path {
        validate_path_text(path, "source media path")?;
    }
    if let Some(path) = &item.source_relative_path {
        validate_relative_path(path)?;
    }
    if item.filename.trim().is_empty()
        || item.filename.len() > 1_024
        || contains_forbidden_control(&item.filename, false)
    {
        return Err(user_error("an imported filename is invalid"));
    }
    if item.file_size > i64::MAX as u64 {
        return Err(user_error("an imported media size is invalid"));
    }
    if !item.duration_seconds.is_finite()
        || item.duration_seconds < 0.0
        || item.duration_seconds > 31_536_000.0
    {
        return Err(user_error("an imported media duration is invalid"));
    }
    if item
        .width
        .is_some_and(|value| value == 0 || value > 100_000)
        || item
            .height
            .is_some_and(|value| value == 0 || value > 100_000)
    {
        return Err(user_error("imported media dimensions are invalid"));
    }
    if item
        .codec
        .as_ref()
        .is_some_and(|codec| codec.len() > 256 || contains_forbidden_control(codec, false))
    {
        return Err(user_error("an imported media codec is invalid"));
    }
    if normalize_tags(&item.tags)? != item.tags {
        return Err(user_error("imported media tags are not normalized"));
    }
    validate_notes(item.notes.as_deref())?;
    if item
        .notes
        .as_ref()
        .is_some_and(|notes| notes.trim().is_empty() || notes.trim() != notes)
    {
        return Err(user_error("imported media notes are not normalized"));
    }
    validate_trim(item)?;
    let mut remote_ids = HashSet::with_capacity(item.remote_assets.len());
    for asset in &item.remote_assets {
        if asset.provider != "youtube"
            || asset.provider_remote_id.is_empty()
            || asset.provider_remote_id.len() > 256
            || contains_forbidden_control(&asset.provider_remote_id, false)
            || !remote_ids.insert((&asset.provider, &asset.provider_remote_id))
            || asset.privacy_status.as_deref() != Some("private")
            || asset.verification_status != "verified"
            || asset.processing_status != "processed"
        {
            return Err(user_error(
                "an imported remote archive reference is invalid",
            ));
        }
        if let Some(browser_url) = &asset.browser_url {
            let parsed = url::Url::parse(browser_url)
                .map_err(|_| user_error("an imported remote archive URL is invalid"))?;
            let allowed_host = parsed.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("youtu.be")
                    || host.eq_ignore_ascii_case("youtube.com")
                    || host.eq_ignore_ascii_case("www.youtube.com")
            });
            if parsed.scheme() != "https" || !allowed_host {
                return Err(user_error("an imported remote archive URL is invalid"));
            }
        }
    }
    Ok(())
}

fn validate_trim(item: &ArchiveItem) -> AppResult<()> {
    for value in [item.trim_start_seconds, item.trim_end_seconds]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() || value < 0.0 || value > item.duration_seconds + 1.0 {
            return Err(user_error("imported clip trim points are invalid"));
        }
    }
    if matches!((item.trim_start_seconds, item.trim_end_seconds), (Some(start), Some(end)) if end <= start)
    {
        return Err(user_error("imported clip trim points are invalid"));
    }
    Ok(())
}

fn match_existing(
    archived: &ArchiveItem,
    current: &[MediaItem],
    by_path: &HashMap<String, usize>,
    roots: &[PathBuf],
) -> Option<usize> {
    let original_key = normalized_path_key(Path::new(&archived.original_path));
    if let Some(index) = by_path.get(&original_key).copied() {
        if signature_matches(archived, &current[index]) {
            return Some(index);
        }
    }
    if let Some(relative) = archived.relative_path.as_deref() {
        let candidates = roots
            .iter()
            .map(|root| root.join(relative))
            .filter_map(|path| by_path.get(&normalized_path_key(&path)).copied())
            .filter(|index| signature_matches(archived, &current[*index]))
            .collect::<HashSet<_>>();
        if candidates.len() == 1 {
            return candidates.into_iter().next();
        }
    }
    let candidates = current
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.filename.eq_ignore_ascii_case(&archived.filename)
                && signature_matches(archived, item)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    (candidates.len() == 1).then(|| candidates[0])
}

fn signature_matches(archived: &ArchiveItem, current: &MediaItem) -> bool {
    archived.kind == current.kind
        && archived.file_size == current.file_size
        && (archived.duration_seconds - current.duration_seconds).abs() <= 1.0
}

fn resolve_existing_media_path(archived: &ArchiveItem, roots: &[PathBuf]) -> Option<PathBuf> {
    let mut paths = vec![PathBuf::from(&archived.original_path)];
    if let Some(relative) = archived.relative_path.as_deref() {
        paths.extend(roots.iter().map(|root| root.join(relative)));
    }
    let matches = paths
        .into_iter()
        .filter(|path| path_is_within_roots(path, roots))
        .filter(|path| {
            path.metadata()
                .map(|metadata| metadata.is_file() && metadata.len() == archived.file_size)
                .unwrap_or(false)
        })
        .map(|path| (normalized_path_key(&path), path))
        .collect::<HashMap<_, _>>();
    if matches.len() == 1 {
        matches.into_values().next()
    } else {
        None
    }
}

fn remote_only_placeholder(data_dir: &Path, archived: &ArchiveItem) -> PathBuf {
    data_dir
        .join("remote-only")
        .join(&archived.archive_id)
        .join(&archived.filename)
}

fn resolve_source_path(archived: &ArchiveItem, roots: &[PathBuf]) -> Option<String> {
    let mut paths = archived
        .original_source_path
        .as_ref()
        .map(PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(relative) = archived.source_relative_path.as_deref() {
        paths.extend(roots.iter().map(|root| root.join(relative)));
    }
    let matches = paths
        .into_iter()
        .filter(|path| path_is_within_roots(path, roots))
        .filter(|path| path.is_file())
        .map(|path| (normalized_path_key(&path), path))
        .collect::<HashMap<_, _>>();
    if matches.len() == 1 {
        matches
            .into_values()
            .next()
            .map(|path| path.to_string_lossy().to_string())
    } else {
        None
    }
}

fn reference_roots(settings: &AppSettings, data_dir: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    settings
        .source_folders
        .iter()
        .map(PathBuf::from)
        .chain(settings.working_folder.as_ref().map(PathBuf::from))
        .chain(settings.clip_folder.as_ref().map(PathBuf::from))
        .chain(std::iter::once(data_dir.join("clips")))
        .filter(|root| seen.insert(normalized_path_key(root)))
        .collect()
}

fn relative_to_roots(path: &Path, roots: &[PathBuf]) -> Option<String> {
    let path_key = normalized_path_key(path);
    roots.iter().find_map(|root| {
        let root_key = normalized_path_key(root);
        path_key
            .strip_prefix(&root_key)
            .and_then(|rest| rest.strip_prefix(crate::media::PATH_KEY_SEPARATOR))
            .filter(|relative| !relative.is_empty())
            .map(str::to_string)
    })
}

fn path_is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    let path_key = normalized_path_key(path);
    roots
        .iter()
        .any(|root| crate::media::path_key_is_within(&path_key, &normalized_path_key(root)))
}

fn merge_tags(existing: &[String], imported: &[String]) -> AppResult<(Vec<String>, bool)> {
    let mut combined = normalize_tags(existing)?;
    let mut seen = combined
        .iter()
        .map(|tag| tag.to_lowercase())
        .collect::<HashSet<_>>();
    let mut bytes = combined.iter().map(String::len).sum::<usize>();
    let mut truncated = false;
    for tag in imported {
        let key = tag.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        if combined.len() == 100 || bytes.saturating_add(tag.len()) > 4_096 {
            truncated = true;
            continue;
        }
        seen.insert(key);
        bytes += tag.len();
        combined.push(tag.clone());
    }
    Ok((combined, truncated))
}

fn normalize_tags(tags: &[String]) -> AppResult<Vec<String>> {
    if tags.len() > 100 {
        return Err(user_error(
            "imported media cannot contain more than 100 tags",
        ));
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 64 || contains_forbidden_control(tag, false) {
            return Err(user_error("an imported media tag is invalid"));
        }
        if seen.insert(tag.to_lowercase()) {
            normalized.push(tag.to_string());
        }
    }
    if normalized.iter().map(String::len).sum::<usize>() > 4_096 {
        return Err(user_error("imported tag data is too large"));
    }
    Ok(normalized)
}

fn validate_notes(notes: Option<&str>) -> AppResult<()> {
    if notes.is_some_and(|notes| notes.len() > 64 * 1024 || contains_forbidden_control(notes, true))
    {
        return Err(user_error("imported media notes are invalid"));
    }
    Ok(())
}

fn validate_collection_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || contains_forbidden_control(name, false) {
        return Err(user_error("an imported collection name is invalid"));
    }
    Ok(name)
}

fn validate_id(id: &str, kind: &str) -> AppResult<()> {
    if id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(user_error(format!(
            "an imported {kind} identifier is invalid"
        )))
    }
}

fn validate_path_text(path: &str, label: &str) -> AppResult<()> {
    let lower = path.to_ascii_lowercase();
    // Archives can come from another OS (e.g. a Windows `C:\\` path imported on
    // Linux). Such paths never resolve locally; they are kept only as lookup keys.
    let foreign = !Path::new(path).is_absolute() && is_foreign_absolute(path);
    let invalid_component = if foreign {
        path.split(['/', '\\'])
            .skip(1)
            .any(|segment| matches!(segment, "." | "..") || segment.contains(':'))
    } else {
        Path::new(path)
            .components()
            .any(|component| match component {
                Component::CurDir | Component::ParentDir => true,
                Component::Normal(value) => value.to_string_lossy().contains(':'),
                _ => false,
            })
    };
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || contains_forbidden_control(path, false)
        || lower.starts_with("\\\\.\\")
        || lower.starts_with("\\\\?\\globalroot")
        || invalid_component
        || !(foreign || Path::new(path).is_absolute())
    {
        return Err(user_error(format!("an imported {label} is invalid")));
    }
    Ok(())
}

/// A Windows drive path (`C:\\…`) or a Unix absolute path (`/…`), regardless of host OS.
fn is_foreign_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

fn validate_relative_path(path: &str) -> AppResult<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || contains_forbidden_control(path, false)
        || path.contains(':')
        || path
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "" | "." | ".."))
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(user_error("an imported relative media path is invalid"));
    }
    Ok(())
}

fn contains_forbidden_control(value: &str, allow_newlines: bool) -> bool {
    value.chars().any(|character| {
        character.is_control() && !(allow_newlines && matches!(character, '\n' | '\r' | '\t'))
    })
}

fn validate_archive_path(path: &Path, must_exist: bool) -> AppResult<()> {
    let text = path.to_string_lossy();
    let lower = text.to_ascii_lowercase();
    if !path.is_absolute()
        || path.as_os_str().is_empty()
        || text.len() > MAX_PATH_BYTES
        || lower.starts_with("\\\\.\\")
        || lower.starts_with("\\\\?\\globalroot")
    {
        return Err(user_error(
            "choose an absolute path for the Reelspace data archive",
        ));
    }
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Err(user_error(
            "Reelspace data archives must use the .json extension",
        ));
    }
    if must_exist {
        if !path.is_file() {
            return Err(user_error(
                "the selected Reelspace data archive does not exist",
            ));
        }
    } else if !path.parent().is_some_and(Path::is_dir) {
        return Err(user_error("the selected export folder does not exist"));
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| user_error("the export filename is invalid"))?;
    let nonce = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let temporary = path.with_file_name(format!(".{filename}.{}.{nonce}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

fn stable_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn invalid_archive(error: serde_json::Error) -> AppError {
    user_error("the selected file contains invalid Reelspace data").with_detail(error.to_string())
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(buffer.len()) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("archive size limit exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("reelspace-portable-{name}-{nonce}"));
        fs::create_dir_all(&directory).expect("fixture");
        directory
    }

    fn media_item(path: &Path, id: char) -> MediaItem {
        MediaItem {
            id: id.to_string().repeat(64),
            kind: MediaKind::Raw,
            path: path.to_string_lossy().to_string(),
            source_path: None,
            filename: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("video.mp4")
                .to_string(),
            file_size: 42,
            duration_seconds: 10.0,
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".into()),
            tags: vec!["highlight".into()],
            notes: Some("portable note".into()),
            thumbnail_path: Some("C:\\private\\thumbnail.jpg".into()),
            created_at: 1,
            modified_at: 2,
            trim_start_seconds: None,
            trim_end_seconds: None,
        }
    }

    fn archive_item() -> ArchiveItem {
        ArchiveItem {
            archive_id: "a".repeat(64),
            kind: MediaKind::Raw,
            original_path: "C:\\captures\\video.mp4".into(),
            relative_path: Some("video.mp4".into()),
            original_source_path: None,
            source_relative_path: None,
            filename: "video.mp4".into(),
            file_size: 42,
            duration_seconds: 10.0,
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".into()),
            tags: vec!["highlight".into()],
            notes: Some("note".into()),
            created_at: 1,
            modified_at: 2,
            trim_start_seconds: None,
            trim_end_seconds: None,
            game_id: None,
            keep_local: None,
            remote_assets: Vec::new(),
        }
    }

    fn archive() -> LibraryArchive {
        LibraryArchive {
            format: ARCHIVE_FORMAT.into(),
            version: ARCHIVE_VERSION,
            exported_at: 1,
            items: vec![archive_item()],
            collections: vec![ArchiveCollection {
                archive_id: "b".repeat(64),
                name: "Favorites".into(),
                member_ids: vec!["a".repeat(64)],
                storage_default: None,
            }],
            games: Vec::new(),
        }
    }

    #[test]
    fn round_trip_remaps_roots_without_copying_media_or_private_cache_paths() {
        let directory = fixture("round-trip");
        let source_root = directory.join("old-root");
        let target_root = directory.join("new-root");
        let relative = PathBuf::from("game").join("video.mp4");
        let source_media = source_root.join(&relative);
        let target_media = target_root.join(&relative);
        fs::create_dir_all(source_media.parent().expect("source parent")).expect("source parent");
        fs::create_dir_all(target_media.parent().expect("target parent")).expect("target parent");
        fs::write(&source_media, vec![7u8; 42]).expect("source media");
        fs::write(&target_media, vec![7u8; 42]).expect("target media");

        let source_db = Database::open(directory.join("source.sqlite")).expect("source database");
        let source_item = media_item(&source_media, 'a');
        source_db.upsert_media(&source_item).expect("source item");
        source_db
            .create_collection(&"b".repeat(64), "Favorites", 1)
            .expect("collection");
        source_db
            .add_to_collection(&"b".repeat(64), &source_item.id, 2)
            .expect("membership");
        source_db
            .set_collection_storage_default(&"b".repeat(64), Some("keepLocal"))
            .expect("collection default");
        source_db.create_game("Hades").expect("game");
        let game_id = source_db.list_games().expect("games")[0].id.clone();
        source_db
            .assign_game(&source_item.id, &game_id)
            .expect("classification");
        source_db
            .rename_game(&game_id, "Hades II", true)
            .expect("rename game");
        source_db
            .set_keep_local(&source_item.id, true)
            .expect("keep local");
        let archive_path = directory.join("library-data.json");
        let export = export_library_data(
            &source_db,
            &AppSettings {
                source_folders: vec![source_root.to_string_lossy().to_string()],
                ..AppSettings::default()
            },
            &directory,
            &archive_path,
        )
        .expect("export");
        assert_eq!(export.items, 1);
        assert_eq!(export.collections, 1);
        let json = fs::read_to_string(&archive_path).expect("archive text");
        assert!(!json.contains("thumbnail.jpg"));
        assert!(!json.contains("thumbnailPath"));
        export_library_data(
            &source_db,
            &AppSettings {
                source_folders: vec![source_root.to_string_lossy().to_string()],
                ..AppSettings::default()
            },
            &directory,
            &archive_path,
        )
        .expect("atomic replacement export");

        let target_db = Database::open(directory.join("target.sqlite")).expect("target database");
        let mut target_item = media_item(&target_media, 'c');
        target_item.tags = vec!["local".into()];
        target_item.notes = None;
        target_item.thumbnail_path = None;
        target_db.upsert_media(&target_item).expect("target item");
        let target_settings = AppSettings {
            source_folders: vec![target_root.to_string_lossy().to_string()],
            ..AppSettings::default()
        };
        let imported = import_library_data(&target_db, &target_settings, &directory, &archive_path)
            .expect("import");
        assert_eq!(imported.matched_items, 1);
        assert_eq!(imported.restored_items, 0);
        assert_eq!(imported.unmatched_items, 0);
        let updated = target_db.get_media(&target_item.id).expect("updated");
        assert_eq!(updated.tags, vec!["local", "highlight"]);
        assert_eq!(updated.notes.as_deref(), Some("portable note"));
        let imported_collection = &target_db.list_collections().expect("collections")[0];
        assert_eq!(imported_collection.item_count, 1);
        assert_eq!(
            imported_collection.storage_default.as_deref(),
            Some("keepLocal")
        );
        let imported_game = &target_db.list_games().expect("imported games")[0];
        assert_eq!(imported_game.name, "Hades II");
        assert!(imported_game.aliases.iter().any(|alias| alias == "Hades"));
        let assignment = &target_db
            .list_game_assignments()
            .expect("imported assignments")[0];
        assert_eq!(assignment.media_id, target_item.id);
        assert!(
            target_db
                .list_archive_statuses()
                .expect("imported preferences")[0]
                .keep_local
        );

        target_db
            .update_notes(&target_item.id, Some("newer local note"))
            .expect("local note");
        let conflict = import_library_data(&target_db, &target_settings, &directory, &archive_path)
            .expect("repeat import");
        assert_eq!(conflict.note_conflicts, 1);
        assert_eq!(
            target_db
                .get_media(&target_item.id)
                .expect("preserved")
                .notes
                .as_deref(),
            Some("newer local note")
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn import_restores_only_records_backed_by_existing_local_media() {
        let directory = fixture("restore");
        let old_root = directory.join("old");
        let new_root = directory.join("new");
        let relative = PathBuf::from("game").join("video.mp4");
        fs::create_dir_all(old_root.join("game")).expect("old root");
        fs::create_dir_all(new_root.join("game")).expect("new root");
        let old_media = old_root.join(&relative);
        let new_media = new_root.join(&relative);
        fs::write(&old_media, vec![1u8; 42]).expect("old media");
        fs::write(&new_media, vec![1u8; 42]).expect("new media");
        let source_db = Database::open(directory.join("source.sqlite")).expect("source database");
        source_db
            .upsert_media(&media_item(&old_media, 'a'))
            .expect("source item");
        let archive_path = directory.join("restore.json");
        export_library_data(
            &source_db,
            &AppSettings {
                source_folders: vec![old_root.to_string_lossy().to_string()],
                ..AppSettings::default()
            },
            &directory,
            &archive_path,
        )
        .expect("export");
        fs::remove_file(old_media).expect("simulate media moved to the new root");

        let restored_db = Database::open(directory.join("restored.sqlite")).expect("restored db");
        let summary = import_library_data(
            &restored_db,
            &AppSettings {
                source_folders: vec![new_root.to_string_lossy().to_string()],
                ..AppSettings::default()
            },
            &directory,
            &archive_path,
        )
        .expect("restore");
        assert_eq!(summary.restored_items, 1);
        assert_eq!(summary.unmatched_items, 0);
        let restored = restored_db.list_media(None).expect("restored media");
        assert_eq!(restored.len(), 1);
        assert_eq!(
            normalized_path_key(Path::new(&restored[0].path)),
            normalized_path_key(&new_media)
        );

        fs::remove_file(new_media).expect("remove local media");
        let empty_db = Database::open(directory.join("empty.sqlite")).expect("empty db");
        let unmatched = import_library_data(
            &empty_db,
            &AppSettings {
                source_folders: vec![new_root.to_string_lossy().to_string()],
                ..AppSettings::default()
            },
            &directory,
            &archive_path,
        )
        .expect("unmatched import");
        assert_eq!(unmatched.unmatched_items, 1);
        assert!(empty_db
            .list_media(None)
            .expect("no phantom media")
            .is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn verified_remote_only_records_survive_without_copying_video_files() {
        let directory = fixture("remote-only");
        let mut archive = archive();
        archive.collections.clear();
        archive.items[0].relative_path = None;
        archive.items[0].remote_assets = vec![ArchiveRemoteAsset {
            provider: "youtube".into(),
            provider_remote_id: "private-video-id".into(),
            browser_url: Some("https://www.youtube.com/watch?v=private-video-id".into()),
            privacy_status: Some("private".into()),
            verification_status: "verified".into(),
            processing_status: "processed".into(),
            verified_at: Some(10),
            last_checked_at: 10,
        }];
        let archive_path = directory.join("remote-only.json");
        fs::write(
            &archive_path,
            serde_json::to_vec_pretty(&archive).expect("serialize archive"),
        )
        .expect("write archive");

        let database = Database::open(directory.join("target.sqlite")).expect("database");
        let summary = import_library_data(
            &database,
            &AppSettings::default(),
            &directory,
            &archive_path,
        )
        .expect("remote-only import");
        assert_eq!(summary.restored_items, 1);
        let media = database.list_media(None).expect("media");
        assert_eq!(media.len(), 1);
        assert!(!Path::new(&media[0].path).exists());
        let status = &database.list_archive_statuses().expect("archive status")[0];
        assert_eq!(status.state, "remoteOnly");
        assert_eq!(status.remote_copy_count, 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn validation_rejects_ambiguous_or_malformed_archive_data() {
        let mut wrong_format = archive();
        wrong_format.format = "not-reelspace".into();
        assert!(validate_archive(&wrong_format).is_err());

        let mut wrong_version = archive();
        wrong_version.version = ARCHIVE_VERSION + 1;
        assert!(validate_archive(&wrong_version).is_err());

        let mut duplicate_id = archive();
        duplicate_id.items.push(duplicate_id.items[0].clone());
        assert!(validate_archive(&duplicate_id).is_err());

        let mut duplicate_path = archive();
        let mut second = duplicate_path.items[0].clone();
        second.archive_id = "c".repeat(64);
        duplicate_path.items.push(second);
        assert!(validate_archive(&duplicate_path).is_err());

        let mut traversal = archive();
        traversal.items[0].relative_path = Some("..\\escape.mp4".into());
        assert!(validate_archive(&traversal).is_err());

        let mut absolute_traversal = archive();
        absolute_traversal.items[0].original_path = "C:\\captures\\..\\private\\escape.mp4".into();
        assert!(validate_archive(&absolute_traversal).is_err());

        let mut alternate_stream = archive();
        alternate_stream.items[0].relative_path = Some("video.mp4:metadata".into());
        assert!(validate_archive(&alternate_stream).is_err());

        let mut device_path = archive();
        device_path.items[0].original_path = "\\\\.\\PhysicalDrive0".into();
        assert!(validate_archive(&device_path).is_err());

        let mut invalid_tags = archive();
        invalid_tags.items[0].tags = vec![" highlight ".into()];
        assert!(validate_archive(&invalid_tags).is_err());

        let mut invalid_notes = archive();
        invalid_notes.items[0].notes = Some(" note ".into());
        assert!(validate_archive(&invalid_notes).is_err());

        let mut invalid_duration = archive();
        invalid_duration.items[0].duration_seconds = f64::NAN;
        assert!(validate_archive(&invalid_duration).is_err());

        let mut invalid_trim = archive();
        invalid_trim.items[0].trim_start_seconds = Some(8.0);
        invalid_trim.items[0].trim_end_seconds = Some(2.0);
        assert!(validate_archive(&invalid_trim).is_err());

        let mut missing_member = archive();
        missing_member.collections[0].member_ids = vec!["d".repeat(64)];
        assert!(validate_archive(&missing_member).is_err());

        let mut duplicate_collection = archive();
        let mut collection = duplicate_collection.collections[0].clone();
        collection.archive_id = "e".repeat(64);
        collection.name = "favorites".into();
        duplicate_collection.collections.push(collection);
        assert!(validate_archive(&duplicate_collection).is_err());
    }

    #[test]
    fn fallback_matching_refuses_ambiguous_signatures() {
        let archived = archive_item();
        let first = media_item(Path::new("C:\\one\\video.mp4"), 'b');
        let second = media_item(Path::new("C:\\two\\video.mp4"), 'c');
        let current = vec![first, second];
        let by_path = current
            .iter()
            .enumerate()
            .map(|(index, item)| (normalized_path_key(Path::new(&item.path)), index))
            .collect();
        assert!(match_existing(&archived, &current, &by_path, &[]).is_none());
    }

    #[test]
    fn record_restoration_never_escapes_current_roots() {
        let directory = fixture("root-containment");
        let outside = directory.join("outside.mp4");
        let configured = directory.join("configured");
        fs::write(&outside, vec![1u8; 42]).expect("outside media");
        fs::create_dir_all(&configured).expect("configured root");
        let mut archived = archive_item();
        archived.original_path = outside.to_string_lossy().to_string();
        archived.relative_path = None;
        assert!(resolve_existing_media_path(&archived, &[configured]).is_none());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn tag_merging_is_non_destructive_at_the_application_limit() {
        let existing = (0..100)
            .map(|index| format!("local-{index}"))
            .collect::<Vec<_>>();
        let imported = vec!["portable".to_string(), "LOCAL-1".to_string()];
        let (merged, truncated) = merge_tags(&existing, &imported).expect("merge");
        assert_eq!(merged, existing);
        assert!(truncated);
    }

    #[test]
    fn archive_file_boundaries_reject_wrong_extensions_and_oversized_input() {
        use crate::error::ErrorCode;

        let directory = fixture("boundaries");
        assert!(validate_archive_path(&directory.join("data.txt"), false).is_err());
        assert!(validate_archive_path(Path::new("relative.json"), false).is_err());
        let oversized = directory.join("oversized.json");
        let file = fs::File::create(&oversized).expect("oversized file");
        file.set_len(MAX_ARCHIVE_BYTES + 1).expect("sparse size");
        drop(file);
        let db = Database::open(directory.join("library.sqlite")).expect("database");
        assert!(import_library_data(&db, &AppSettings::default(), &directory, &oversized).is_err());
        let malformed = directory.join("malformed.json");
        fs::write(&malformed, b"{ definitely not json").expect("malformed archive");
        let error = import_library_data(&db, &AppSettings::default(), &directory, &malformed)
            .expect_err("malformed archive");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        let mut writer = BoundedWriter::new(4);
        assert!(writer.write_all(b"12345").is_err());
        assert!(writer.exceeded);
        assert!(writer.bytes.is_empty());
        let _ = fs::remove_dir_all(directory);
    }
}
