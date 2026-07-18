use crate::db::Database;
use crate::error::{app_error, user_error, AppResult, ErrorCode};
use crate::models::{
    ArchiveStatus, Game, GameAssignment, MediaEvent, MediaItem, RecorderApp, RepairSummary,
    UploadJob,
};
use crate::state::AppState;
use crate::windows_integration::open_external_url;
use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::State;

const GAME_COLORS: [&str; 12] = [
    "#7dd3fc", "#b9a7ff", "#6ee7b7", "#f6c56f", "#fb8c8c", "#67e8f9", "#c4b5fd", "#86efac",
    "#fdba74", "#f9a8d4", "#93c5fd", "#d8b4fe",
];

impl Database {
    pub fn initialize_clip_preferences(
        &self,
        source_id: &str,
        clip_id: &str,
        keep_local: bool,
    ) -> AppResult<()> {
        let conn = self.conn()?;
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT OR IGNORE INTO media_classification (media_id, game_id, source_value, assigned_at)
             SELECT ?1, game_id, source_value, ?3 FROM media_classification WHERE media_id = ?2",
            params![clip_id, source_id, now],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO media_preferences (media_id, keep_local, updated_at) VALUES (?1, ?2, ?3)",
            params![clip_id, keep_local, now],
        )?;
        Ok(())
    }

    pub fn classify_media_items(&self, items: &[MediaItem]) -> AppResult<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for item in items {
            let Some(label) = game_label_for_path(Path::new(&item.path)) else {
                continue;
            };
            let key = normalized_label(&label);
            if key.is_empty() {
                continue;
            }
            let game_id: Option<String> = tx
                .query_row(
                    "SELECT game_id FROM game_aliases WHERE alias_key = ?1",
                    [&key],
                    |row| row.get(0),
                )
                .optional()?;
            let game_id = match game_id {
                Some(id) => id,
                None => {
                    let existing: Option<String> = tx
                        .query_row(
                            "SELECT id FROM games WHERE name = ?1 COLLATE NOCASE",
                            [&label],
                            |row| row.get(0),
                        )
                        .optional()?;
                    existing.unwrap_or_else(|| stable_id(&format!("game:{key}")))
                }
            };
            tx.execute(
                "INSERT OR IGNORE INTO games (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![game_id, label, color_for(&key), Utc::now().timestamp()],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO game_aliases (alias_key, source_label, game_id, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![key, label, game_id, Utc::now().timestamp()],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO media_classification (media_id, game_id, source_value, assigned_at) VALUES (?1, ?2, ?3, ?4)",
                params![item.id, game_id, label, Utc::now().timestamp()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_games(&self) -> AppResult<Vec<Game>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT g.id, g.name, g.color, COUNT(DISTINCT mc.media_id)
             FROM games g LEFT JOIN media_classification mc ON mc.game_id = g.id
             GROUP BY g.id ORDER BY g.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? as u64,
            ))
        })?;
        let mut games = Vec::new();
        for row in rows {
            let (id, name, color, media_count) = row?;
            let mut aliases_stmt = conn.prepare(
                "SELECT source_label FROM game_aliases WHERE game_id = ?1 ORDER BY source_label COLLATE NOCASE",
            )?;
            let aliases = aliases_stmt
                .query_map([&id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            games.push(Game {
                id,
                name,
                color,
                media_count,
                aliases,
            });
        }
        Ok(games)
    }

    pub fn create_game(&self, name: &str) -> AppResult<()> {
        let name = validated_game_name(name)?;
        let key = normalized_label(name);
        let id = stable_id(&format!("game:{key}"));
        let now = Utc::now().timestamp();
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO games (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, color_for(&key), now],
        )?;
        tx.execute("INSERT INTO game_aliases (alias_key, source_label, game_id, created_at) VALUES (?1, ?2, ?3, ?4)", params![key, name, id, now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_game_assignments(&self) -> AppResult<Vec<GameAssignment>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT mc.media_id, g.id, g.name, g.color FROM media_classification mc INNER JOIN games g ON g.id = mc.game_id",
        )?;
        let values = stmt
            .query_map([], |row| {
                Ok(GameAssignment {
                    media_id: row.get(0)?,
                    game_id: row.get(1)?,
                    game_name: row.get(2)?,
                    color: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(values)
    }

    pub fn rename_game(&self, id: &str, name: &str, preserve_alias: bool) -> AppResult<()> {
        let name = validated_game_name(name)?;
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: String = tx
            .query_row("SELECT name FROM games WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| user_error("game was not found"))?;
        tx.execute(
            "UPDATE games SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if preserve_alias {
            tx.execute(
                "INSERT OR REPLACE INTO game_aliases (alias_key, source_label, game_id, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![normalized_label(&old), old, id, Utc::now().timestamp()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn merge_games(&self, source_id: &str, target_id: &str) -> AppResult<()> {
        if source_id == target_id {
            return Err(user_error("a game cannot be merged into itself"));
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_name: String = tx
            .query_row("SELECT name FROM games WHERE id = ?1", [source_id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| user_error("source game was not found"))?;
        let target_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
            [target_id],
            |row| row.get(0),
        )?;
        if !target_exists {
            return Err(user_error("target game was not found"));
        }
        tx.execute(
            "UPDATE media_classification SET game_id = ?1 WHERE game_id = ?2",
            params![target_id, source_id],
        )?;
        tx.execute(
            "UPDATE game_aliases SET game_id = ?1 WHERE game_id = ?2",
            params![target_id, source_id],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO game_aliases (alias_key, source_label, game_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![normalized_label(&source_name), source_name, target_id, Utc::now().timestamp()],
        )?;
        tx.execute("DELETE FROM games WHERE id = ?1", [source_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn assign_game(&self, media_id: &str, game_id: &str) -> AppResult<()> {
        let conn = self.conn()?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
            [game_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(user_error("game was not found"));
        }
        let changed = conn.execute(
            "INSERT INTO media_classification (media_id, game_id, source_value, assigned_at) VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT(media_id) DO UPDATE SET game_id = excluded.game_id, assigned_at = excluded.assigned_at",
            params![media_id, game_id, Utc::now().timestamp()],
        )?;
        if changed != 1 {
            return Err(user_error("media item was not found"));
        }
        Ok(())
    }

    pub fn list_archive_statuses(&self) -> AppResult<Vec<ArchiveStatus>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.path, m.kind, COALESCE(mp.keep_local, 0),
                    COUNT(DISTINCT ra.id),
                    MAX(CASE WHEN ra.verification_status = 'verified' THEN 1 ELSE 0 END),
                    MAX(ra.browser_url),
                    COALESCE((SELECT uj.state FROM upload_jobs uj WHERE uj.media_id = m.id AND uj.state NOT IN ('canceled') ORDER BY uj.updated_at DESC LIMIT 1), ''),
                    COALESCE((SELECT uj.bytes_uploaded FROM upload_jobs uj WHERE uj.media_id = m.id ORDER BY uj.updated_at DESC LIMIT 1), 0),
                    COALESCE((SELECT uj.total_bytes FROM upload_jobs uj WHERE uj.media_id = m.id ORDER BY uj.updated_at DESC LIMIT 1), 0),
                    (SELECT uj.error FROM upload_jobs uj WHERE uj.media_id = m.id ORDER BY uj.updated_at DESC LIMIT 1)
             FROM media m
             LEFT JOIN media_preferences mp ON mp.media_id = m.id
             LEFT JOIN remote_assets ra ON ra.media_id = m.id
             GROUP BY m.id",
        )?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let mut keep_local = row.get::<_, i64>(3)? != 0;
            if kind == "clip" && !keep_local {
                keep_local = true;
            }
            let copies = row.get::<_, i64>(4)? as u64;
            let verified = row.get::<_, i64>(5)? != 0;
            let job: String = row.get(7)?;
            let uploaded = row.get::<_, i64>(8)?.max(0) as u64;
            let total = row.get::<_, i64>(9)?.max(0) as u64;
            let state = archive_state(&job, copies, verified, Path::new(&path).is_file());
            Ok(ArchiveStatus {
                media_id: row.get(0)?,
                state,
                keep_local,
                remote_copy_count: copies,
                primary_remote_url: row.get(6)?,
                progress_percent: uploaded
                    .saturating_mul(100)
                    .checked_div(total)
                    .unwrap_or(0)
                    .min(100) as u8,
                error: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_keep_local(&self, media_id: &str, keep_local: bool) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO media_preferences (media_id, keep_local, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(media_id) DO UPDATE SET keep_local = excluded.keep_local, updated_at = excluded.updated_at",
            params![media_id, keep_local, Utc::now().timestamp()],
        )?;
        drop(conn);
        self.record_media_event(
            media_id,
            if keep_local {
                "keep_local_enabled"
            } else {
                "keep_local_disabled"
            },
            None,
        )
    }

    pub fn queue_archive(&self, media_ids: &[String]) -> AppResult<u64> {
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now().timestamp();
        let mut queued = 0;
        for (index, media_id) in media_ids.iter().enumerate() {
            let media: Option<(String, i64)> = tx
                .query_row(
                    "SELECT path, file_size FROM media WHERE id = ?1",
                    [media_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((path, size)) = media else {
                return Err(user_error("media item was not found"));
            };
            if !Path::new(&path).is_file() {
                return Err(user_error("remote-only media cannot be queued for upload"));
            }
            let already_verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM remote_assets WHERE media_id = ?1 AND verification_status = 'verified')", [media_id], |row| row.get(0))?;
            if already_verified {
                continue;
            }
            let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM upload_jobs WHERE media_id = ?1 AND state NOT IN ('failed', 'canceled', 'archived'))", [media_id], |row| row.get(0))?;
            if active {
                continue;
            }
            let id = stable_id(&format!("upload:{media_id}:{now}:{index}"));
            tx.execute(
                "INSERT INTO upload_jobs (id, media_id, provider, state, priority, total_bytes, created_at, updated_at) VALUES (?1, ?2, 'youtube', 'queued', ?3, ?4, ?5, ?5)",
                params![id, media_id, if media_ids.len() == 1 { 100 } else { 10 }, size.max(0), now],
            )?;
            tx.execute("INSERT INTO media_events (media_id, event, created_at) VALUES (?1, 'queued_for_archive', ?2)", params![media_id, now])?;
            queued += 1;
        }
        tx.commit()?;
        Ok(queued)
    }

    pub fn list_upload_jobs(&self) -> AppResult<Vec<UploadJob>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT uj.id, uj.media_id, m.filename, uj.state, uj.priority, uj.bytes_uploaded, uj.total_bytes, uj.attempts, uj.error, uj.updated_at
             FROM upload_jobs uj INNER JOIN media m ON m.id = uj.media_id
             WHERE uj.state != 'canceled' ORDER BY CASE uj.state WHEN 'uploading' THEN 0 WHEN 'preparing' THEN 1 WHEN 'queued' THEN 2 ELSE 3 END, uj.priority DESC, uj.created_at",
        )?;
        let values = stmt
            .query_map([], |row| {
                Ok(UploadJob {
                    id: row.get(0)?,
                    media_id: row.get(1)?,
                    filename: row.get(2)?,
                    state: row.get(3)?,
                    priority: row.get(4)?,
                    bytes_uploaded: row.get::<_, i64>(5)?.max(0) as u64,
                    total_bytes: row.get::<_, i64>(6)?.max(0) as u64,
                    attempts: row.get::<_, i64>(7)?.max(0) as u32,
                    error: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(values)
    }

    pub fn control_upload_job(&self, action: &str, job_id: Option<&str>) -> AppResult<()> {
        let conn = self.conn()?;
        let now = Utc::now().timestamp();
        let (state, where_clause) = match action {
            "pauseAll" => (
                "paused",
                "state IN ('queued', 'retry', 'waiting_for_quota')",
            ),
            "resumeAll" => ("queued", "state = 'paused'"),
            "pause" => (
                "paused",
                "id = ?1 AND state IN ('queued', 'retry', 'waiting_for_quota')",
            ),
            "resume" | "retry" => (
                "queued",
                "id = ?1 AND state IN ('paused', 'failed', 'waiting_for_quota')",
            ),
            "cancel" => (
                "canceled",
                "id = ?1 AND state NOT IN ('archived', 'uploading')",
            ),
            _ => return Err(user_error("unknown upload queue action")),
        };
        let sql = format!("UPDATE upload_jobs SET state = '{state}', error = NULL, next_attempt_at = 0, updated_at = {now} WHERE {where_clause}");
        if action.ends_with("All") {
            conn.execute(&sql, [])?;
        } else {
            conn.execute(
                &sql,
                [job_id.ok_or_else(|| user_error("job id is required"))?],
            )?;
        }
        Ok(())
    }

    pub fn has_verified_remote(&self, media_id: &str) -> AppResult<bool> {
        let conn = self.conn()?;
        conn.query_row("SELECT EXISTS(SELECT 1 FROM remote_assets WHERE media_id = ?1 AND verification_status = 'verified')", [media_id], |row| row.get(0)).map_err(Into::into)
    }

    pub fn record_media_event(
        &self,
        media_id: &str,
        event: &str,
        detail: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute("INSERT INTO media_events (media_id, event, detail, created_at) VALUES (?1, ?2, ?3, ?4)", params![media_id, event, detail, Utc::now().timestamp()])?;
        Ok(())
    }

    pub fn list_media_events(&self, media_id: &str) -> AppResult<Vec<MediaEvent>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT event, detail, created_at FROM media_events WHERE media_id = ?1 ORDER BY created_at DESC LIMIT 100")?;
        let values = stmt
            .query_map([media_id], |row| {
                Ok(MediaEvent {
                    event: row.get(0)?,
                    detail: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(values)
    }

    pub fn remove_from_collection(&self, collection_id: &str, media_id: &str) -> AppResult<()> {
        self.conn()?.execute(
            "DELETE FROM collection_items WHERE collection_id = ?1 AND media_id = ?2",
            params![collection_id, media_id],
        )?;
        Ok(())
    }

    pub fn collection_storage_default(&self, collection_id: &str) -> AppResult<Option<String>> {
        self.conn()?
            .query_row(
                "SELECT CASE
                   WHEN keep_local_default = 1 THEN 'keepLocal'
                   WHEN remote_only_default = 1 THEN 'archive'
                   ELSE NULL
                 END
                 FROM collection_preferences WHERE collection_id = ?1",
                [collection_id],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(Into::into)
    }

    pub fn set_collection_storage_default(
        &self,
        collection_id: &str,
        policy: Option<&str>,
    ) -> AppResult<()> {
        if !matches!(policy, None | Some("keepLocal") | Some("archive")) {
            return Err(user_error("unknown collection storage default"));
        }
        let mut conn = self.conn()?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM collections WHERE id = ?1)",
            [collection_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(user_error("collection was not found"));
        }
        match policy {
            None => {
                transaction.execute(
                    "DELETE FROM collection_preferences WHERE collection_id = ?1",
                    [collection_id],
                )?;
            }
            Some(policy) => {
                let keep_local = policy == "keepLocal";
                transaction.execute(
                    "INSERT INTO collection_preferences (collection_id, keep_local_default, remote_only_default, updated_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(collection_id) DO UPDATE SET
                       keep_local_default = excluded.keep_local_default,
                       remote_only_default = excluded.remote_only_default,
                       updated_at = excluded.updated_at",
                    params![collection_id, keep_local, !keep_local, Utc::now().timestamp()],
                )?;
                transaction.execute(
                    "INSERT INTO media_preferences (media_id, keep_local, updated_at)
                     SELECT media_id, ?2, ?3 FROM collection_items WHERE collection_id = ?1
                     ON CONFLICT(media_id) DO UPDATE SET keep_local = excluded.keep_local, updated_at = excluded.updated_at",
                    params![collection_id, keep_local, Utc::now().timestamp()],
                )?;
            }
        }
        transaction.commit()?;
        if policy == Some("archive") {
            let media_ids = self
                .list_collection_media(collection_id)?
                .into_iter()
                .filter(|item| Path::new(&item.path).is_file())
                .map(|item| item.id)
                .collect::<Vec<_>>();
            if !media_ids.is_empty() {
                self.queue_archive(&media_ids)?;
            }
        }
        Ok(())
    }
}

#[tauri::command]
pub fn list_games(state: State<'_, AppState>) -> AppResult<Vec<Game>> {
    state.db.list_games()
}

#[tauri::command]
pub fn create_game(state: State<'_, AppState>, name: String) -> AppResult<Vec<Game>> {
    state.db.create_game(&name)?;
    state.db.list_games()
}

#[tauri::command]
pub fn list_game_assignments(state: State<'_, AppState>) -> AppResult<Vec<GameAssignment>> {
    state.db.list_game_assignments()
}

#[tauri::command]
pub fn rename_game(
    state: State<'_, AppState>,
    id: String,
    name: String,
    preserve_alias: bool,
) -> AppResult<Vec<Game>> {
    state.db.rename_game(&id, &name, preserve_alias)?;
    state.db.list_games()
}

#[tauri::command]
pub fn merge_games(
    state: State<'_, AppState>,
    source_id: String,
    target_id: String,
) -> AppResult<Vec<Game>> {
    state.db.merge_games(&source_id, &target_id)?;
    state.db.list_games()
}

#[tauri::command]
pub fn assign_game(
    state: State<'_, AppState>,
    media_id: String,
    game_id: String,
) -> AppResult<Vec<GameAssignment>> {
    state.db.assign_game(&media_id, &game_id)?;
    state.db.list_game_assignments()
}

#[tauri::command]
pub fn list_archive_statuses(state: State<'_, AppState>) -> AppResult<Vec<ArchiveStatus>> {
    state.db.list_archive_statuses()
}

#[tauri::command]
pub fn set_keep_local(
    state: State<'_, AppState>,
    media_id: String,
    keep_local: bool,
) -> AppResult<Vec<ArchiveStatus>> {
    state.db.set_keep_local(&media_id, keep_local)?;
    state.db.list_archive_statuses()
}

#[tauri::command]
pub fn queue_archive(state: State<'_, AppState>, media_ids: Vec<String>) -> AppResult<u64> {
    if media_ids.is_empty() || media_ids.len() > 10_000 {
        return Err(user_error("select between 1 and 10,000 media items"));
    }
    state.db.queue_archive(&media_ids)
}

#[tauri::command]
pub fn list_upload_jobs(state: State<'_, AppState>) -> AppResult<Vec<UploadJob>> {
    state.db.list_upload_jobs()
}

#[tauri::command]
pub fn control_upload_queue(
    state: State<'_, AppState>,
    action: String,
    job_id: Option<String>,
) -> AppResult<Vec<UploadJob>> {
    state.db.control_upload_job(&action, job_id.as_deref())?;
    state.db.list_upload_jobs()
}

#[tauri::command]
pub fn list_media_events(
    state: State<'_, AppState>,
    media_id: String,
) -> AppResult<Vec<MediaEvent>> {
    state.db.list_media_events(&media_id)
}

#[tauri::command]
pub fn remove_from_collection(
    state: State<'_, AppState>,
    collection_id: String,
    media_id: String,
) -> AppResult<()> {
    state.db.remove_from_collection(&collection_id, &media_id)
}

#[tauri::command]
pub fn set_collection_storage_default(
    state: State<'_, AppState>,
    collection_id: String,
    policy: Option<String>,
) -> AppResult<Vec<crate::models::Collection>> {
    state
        .db
        .set_collection_storage_default(&collection_id, policy.as_deref())?;
    state.db.list_collections()
}

#[tauri::command]
pub fn detect_recording_apps(state: State<'_, AppState>) -> AppResult<Vec<RecorderApp>> {
    let settings = state.db.load_settings()?;
    let mut candidates = recorder_candidates();
    if let Some(custom) = settings.preferred_recording_app {
        candidates.insert(
            0,
            (
                "custom".into(),
                "Preferred recorder".into(),
                PathBuf::from(custom),
            ),
        );
    }
    let mut seen = HashMap::new();
    Ok(candidates
        .into_iter()
        .filter(|(_, _, path)| path.is_file())
        .filter_map(|(id, name, path)| {
            let key = path.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key, ()).is_some() {
                return None;
            }
            Some(RecorderApp {
                id,
                name,
                executable_path: path.to_string_lossy().into_owned(),
            })
        })
        .collect())
}

#[tauri::command]
pub fn launch_recording_app(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let apps = detect_recording_apps(state)?;
    let app = apps
        .into_iter()
        .find(|app| app.id == id)
        .ok_or_else(|| user_error("recording application is no longer available"))?;
    std::process::Command::new(&app.executable_path)
        .spawn()
        .map_err(|error| {
            app_error(
                ErrorCode::WindowsIntegrationFailed,
                "The recording application could not be launched",
            )
            .with_detail(error.to_string())
        })?;
    Ok(())
}

#[tauri::command]
pub fn remove_local_copy(
    state: State<'_, AppState>,
    media_id: String,
) -> AppResult<Vec<ArchiveStatus>> {
    let item = state.db.get_media(&media_id)?;
    if !state.db.has_verified_remote(&media_id)? {
        return Err(user_error(
            "a verified remote copy is required before local cleanup",
        ));
    }
    let statuses = state.db.list_archive_statuses()?;
    if statuses
        .iter()
        .find(|status| status.media_id == media_id)
        .is_some_and(|status| status.keep_local)
    {
        return Err(user_error("disable Keep Local before removing this copy"));
    }
    let path = Path::new(&item.path);
    if !path.is_file() {
        return Err(user_error("the local copy is already unavailable"));
    }
    trash::delete(path).map_err(|error| {
        app_error(
            ErrorCode::PermissionDenied,
            "The local copy could not be moved to the Recycle Bin",
        )
        .with_detail(error.to_string())
    })?;
    state.db.record_media_event(
        &media_id,
        "local_copy_removed",
        Some("Moved to Windows Recycle Bin"),
    )?;
    state.db.list_archive_statuses()
}

#[tauri::command]
pub fn open_remote_copy(state: State<'_, AppState>, media_id: String) -> AppResult<()> {
    let status = state
        .db
        .list_archive_statuses()?
        .into_iter()
        .find(|status| status.media_id == media_id)
        .ok_or_else(|| user_error("media item was not found"))?;
    let url = status
        .primary_remote_url
        .ok_or_else(|| user_error("no remote browser link is available"))?;
    open_external_url(&url, "The remote copy could not be opened")
}

#[tauri::command]
pub fn repair_library(state: State<'_, AppState>) -> AppResult<RepairSummary> {
    let items = state.db.list_media(None)?;
    let mut summary = RepairSummary::default();
    for item in items {
        if !Path::new(&item.path).is_file() {
            summary.missing_local_files += 1;
            if state.db.has_verified_remote(&item.id)? {
                summary.preserved_remote_only_items += 1;
            }
        }
    }
    summary.stale_jobs_recovered = state
        .db
        .list_upload_jobs()?
        .iter()
        .filter(|job| job.error.as_deref() == Some("Recovered after Reelspace restarted"))
        .count() as u64;
    Ok(summary)
}

#[tauri::command]
pub fn open_diagnostics_folder(state: State<'_, AppState>) -> AppResult<()> {
    let path = state.data_dir.join("logs");
    std::fs::create_dir_all(&path)?;
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| {
            app_error(
                ErrorCode::WindowsIntegrationFailed,
                "The diagnostics folder could not be opened",
            )
            .with_detail(error.to_string())
        })?;
    Ok(())
}

#[tauri::command]
pub fn export_diagnostics(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let destination = Path::new(&path);
    if !destination.is_absolute()
        || destination
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("json"))
    {
        return Err(user_error(
            "diagnostics must be exported to an absolute .json path",
        ));
    }
    let settings = state.db.load_settings()?;
    let media = state.db.list_media(None)?;
    let jobs = state.db.list_upload_jobs()?;
    let report = serde_json::json!({
        "schemaVersion": 1,
        "generatedAt": Utc::now().to_rfc3339(),
        "reelspaceVersion": env!("CARGO_PKG_VERSION"),
        "platform": "windows-x64",
        "sourceFolderCount": settings.source_folders.len(),
        "mediaItems": media.len(),
        "queuedArchiveJobs": jobs.iter().filter(|job| job.state != "archived").count(),
        "ffmpegReady": state.ffmpeg.health().ready,
        "activeMediaWork": state.media_jobs.active_count() > 0 || state.export_jobs.active_count() > 0 || state.background_jobs.active_count() > 0,
        "privacy": "No usernames, local paths, OAuth tokens, video titles, or file names are included."
    });
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(destination, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn recorder_candidates() -> Vec<(String, String, PathBuf)> {
    let program_files = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    vec![
        (
            "obs".into(),
            "OBS Studio".into(),
            program_files.join(r"obs-studio\bin\64bit\obs64.exe"),
        ),
        (
            "medal".into(),
            "Medal".into(),
            local.join(r"Medal\Medal.exe"),
        ),
        (
            "outplayed".into(),
            "Outplayed".into(),
            local.join(r"Overwolf\Outplayed\Outplayed.exe"),
        ),
        (
            "xbox".into(),
            "Xbox Game Bar".into(),
            PathBuf::from(r"C:\Windows\System32\GameBar.exe"),
        ),
        (
            "shadowplay".into(),
            "NVIDIA App".into(),
            program_files.join(r"NVIDIA Corporation\NVIDIA App\CEF\NVIDIA App.exe"),
        ),
        (
            "shadowplay-legacy".into(),
            "NVIDIA Share".into(),
            program_files_x86
                .join(r"NVIDIA Corporation\NVIDIA GeForce Experience\NVIDIA Share.exe"),
        ),
    ]
}

fn game_label_for_path(path: &Path) -> Option<String> {
    let generic = [
        "clips",
        "captures",
        "recordings",
        "videos",
        "medal",
        "shadowplay",
    ];
    let mut parent = path.parent()?;
    let mut label = parent.file_name()?.to_string_lossy().trim().to_owned();
    if generic
        .iter()
        .any(|value| label.eq_ignore_ascii_case(value))
    {
        parent = parent.parent()?;
        label = parent.file_name()?.to_string_lossy().trim().to_owned();
    }
    (!label.is_empty()).then_some(label)
}

fn validated_game_name(name: &str) -> AppResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 || trimmed.chars().any(|ch| ch.is_control()) {
        return Err(user_error(
            "game name must contain 1 to 120 visible characters",
        ));
    }
    Ok(trimmed)
}

fn normalized_label(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn stable_id(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn color_for(key: &str) -> &'static str {
    GAME_COLORS[(Sha256::digest(key.as_bytes())[0] as usize) % GAME_COLORS.len()]
}

fn archive_state(job: &str, copies: u64, verified: bool, local: bool) -> String {
    if copies > 1 && verified {
        return "multipleRemoteCopies".into();
    }
    match job {
        "queued" | "retry" | "paused" => "queued".into(),
        "preparing" | "verifying" => "preparing".into(),
        "uploading" => "uploading".into(),
        "waiting_for_quota" => "waitingForQuota".into(),
        "processing_remote" => "processingRemote".into(),
        "failed" | "needs_sign_in" => "uploadFailed".into(),
        "remote_missing" => "remoteMissing".into(),
        _ if verified && local => "archivedVerified".into(),
        _ if verified => "remoteOnly".into(),
        _ => "localOnly".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_labels_ignore_generic_capture_directories() {
        assert_eq!(
            game_label_for_path(Path::new(r"D:\Medal\Once Human\Clips\capture.mp4")).as_deref(),
            Some("Once Human")
        );
    }
    #[test]
    fn archive_states_are_explicit_and_non_color_dependent() {
        assert_eq!(
            archive_state("waiting_for_quota", 0, false, true),
            "waitingForQuota"
        );
        assert_eq!(archive_state("", 1, true, false), "remoteOnly");
        assert_eq!(archive_state("", 2, true, true), "multipleRemoteCopies");
    }
}
