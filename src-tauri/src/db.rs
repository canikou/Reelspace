use crate::error::{app_error, user_error, AppResult, ErrorCode};
use crate::media::FileFingerprint;
use crate::models::{AppSettings, Collection, MediaItem, MediaKind, ProbeData};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

const DATABASE_SCHEMA_VERSION: i64 = 2;
pub const PROBE_SCHEMA_VERSION: i64 = 1;

impl Database {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::migrate(&mut conn)?;
        let integrity: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(app_error(
                ErrorCode::DatabaseIntegrityFailed,
                "The local media library failed an integrity check",
            )
            .with_detail(integrity));
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| user_error("database connection was poisoned"))
    }

    fn migrate(conn: &mut Connection) -> AppResult<()> {
        let current_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| {
                app_error(
                    ErrorCode::DatabaseMigrationFailed,
                    "The media library version could not be read",
                )
                .with_detail(error.to_string())
            })?;
        if current_version > DATABASE_SCHEMA_VERSION {
            return Err(app_error(
                ErrorCode::DatabaseMigrationFailed,
                "This media library was created by a newer Reelspace version",
            ));
        }
        let transaction = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| {
                app_error(
                    ErrorCode::DatabaseMigrationFailed,
                    "The media library migration could not start",
                )
                .with_detail(error.to_string())
            })?;
        transaction
            .execute_batch(
                r#"
            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY NOT NULL,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS media (
              id TEXT PRIMARY KEY NOT NULL,
              kind TEXT NOT NULL,
              path TEXT NOT NULL UNIQUE,
              source_path TEXT,
              filename TEXT NOT NULL,
              file_size INTEGER NOT NULL,
              duration_seconds REAL NOT NULL,
              width INTEGER,
              height INTEGER,
              codec TEXT,
              tags_json TEXT NOT NULL,
              notes TEXT,
              thumbnail_path TEXT,
              created_at INTEGER NOT NULL,
              modified_at INTEGER NOT NULL,
              trim_start_seconds REAL,
              trim_end_seconds REAL
            );

            CREATE TABLE IF NOT EXISTS collections (
              id TEXT PRIMARY KEY NOT NULL,
              name TEXT NOT NULL UNIQUE,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collection_items (
              collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
              media_id TEXT NOT NULL REFERENCES media(id) ON DELETE CASCADE,
              added_at INTEGER NOT NULL,
              PRIMARY KEY (collection_id, media_id)
            );

            CREATE INDEX IF NOT EXISTS idx_media_kind_modified ON media(kind, modified_at DESC);
            CREATE INDEX IF NOT EXISTS idx_media_source_path ON media(source_path);

            CREATE TABLE IF NOT EXISTS probe_cache (
              path_key TEXT PRIMARY KEY NOT NULL,
              file_size INTEGER NOT NULL,
              modified_ns TEXT NOT NULL,
              probe_version INTEGER NOT NULL,
              probe_json TEXT NOT NULL,
              probed_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_probe_cache_fingerprint
              ON probe_cache(path_key, file_size, modified_ns, probe_version);
            "#,
            )
            .map_err(|error| {
                app_error(
                    ErrorCode::DatabaseMigrationFailed,
                    "The media library schema could not be migrated",
                )
                .with_detail(error.to_string())
            })?;
        transaction
            .pragma_update(None, "user_version", DATABASE_SCHEMA_VERSION)
            .map_err(|error| {
                app_error(
                    ErrorCode::DatabaseMigrationFailed,
                    "The media library version could not be updated",
                )
                .with_detail(error.to_string())
            })?;
        transaction.commit().map_err(|error| {
            app_error(
                ErrorCode::DatabaseMigrationFailed,
                "The media library migration could not be committed",
            )
            .with_detail(error.to_string())
        })?;
        Ok(())
    }

    pub fn get_probe_cache(&self, fingerprint: &FileFingerprint) -> AppResult<Option<ProbeData>> {
        let conn = self.conn()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT probe_json FROM probe_cache
                 WHERE path_key = ?1 AND file_size = ?2 AND modified_ns = ?3 AND probe_version = ?4",
                params![
                    fingerprint.path_key,
                    fingerprint.size as i64,
                    fingerprint.modified_ns.to_string(),
                    PROBE_SCHEMA_VERSION,
                ],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = json else {
            return Ok(None);
        };
        match serde_json::from_str::<ProbeData>(&json) {
            Ok(data) if probe_data_is_valid(&data) => Ok(Some(data)),
            _ => {
                conn.execute(
                    "DELETE FROM probe_cache WHERE path_key = ?1",
                    [&fingerprint.path_key],
                )?;
                Ok(None)
            }
        }
    }

    pub fn put_probe_cache(
        &self,
        fingerprint: &FileFingerprint,
        data: &ProbeData,
    ) -> AppResult<()> {
        if !probe_data_is_valid(data) {
            return Err(app_error(
                ErrorCode::CorruptMedia,
                "Invalid media metadata was not cached",
            ));
        }
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO probe_cache (
               path_key, file_size, modified_ns, probe_version, probe_json, probed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path_key) DO UPDATE SET
               file_size = excluded.file_size,
               modified_ns = excluded.modified_ns,
               probe_version = excluded.probe_version,
               probe_json = excluded.probe_json,
               probed_at = excluded.probed_at",
            params![
                fingerprint.path_key,
                fingerprint.size as i64,
                fingerprint.modified_ns.to_string(),
                PROBE_SCHEMA_VERSION,
                serde_json::to_string(data)?,
                Utc::now().timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn probe_cache_count(&self) -> AppResult<u64> {
        let conn = self.conn()?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM probe_cache", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    pub fn load_settings(&self) -> AppResult<AppSettings> {
        let conn = self.conn()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'app_settings'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match value {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app_settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn upsert_media(&self, item: &MediaItem) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            r#"
            INSERT INTO media (
              id, kind, path, source_path, filename, file_size, duration_seconds,
              width, height, codec, tags_json, notes, thumbnail_path, created_at,
              modified_at, trim_start_seconds, trim_end_seconds
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(path) DO UPDATE SET
              kind = CASE
                WHEN media.kind = 'clip' AND excluded.kind = 'raw' THEN media.kind
                ELSE excluded.kind
              END,
              source_path = CASE
                WHEN media.kind = 'clip' AND excluded.kind = 'raw' THEN media.source_path
                ELSE excluded.source_path
              END,
              filename = excluded.filename,
              file_size = excluded.file_size,
              duration_seconds = excluded.duration_seconds,
              width = excluded.width,
              height = excluded.height,
              codec = excluded.codec,
              tags_json = CASE WHEN media.tags_json = '[]' THEN excluded.tags_json ELSE media.tags_json END,
              thumbnail_path = COALESCE(excluded.thumbnail_path, media.thumbnail_path),
              notes = CASE
                WHEN media.kind = 'clip' AND excluded.kind = 'raw' THEN media.notes
                ELSE excluded.notes
              END,
              modified_at = excluded.modified_at,
              trim_start_seconds = CASE
                WHEN media.kind = 'clip' AND excluded.kind = 'raw' THEN media.trim_start_seconds
                ELSE excluded.trim_start_seconds
              END,
              trim_end_seconds = CASE
                WHEN media.kind = 'clip' AND excluded.kind = 'raw' THEN media.trim_end_seconds
                ELSE excluded.trim_end_seconds
              END
            "#,
            params![
                item.id,
                item.kind.as_str(),
                item.path,
                item.source_path,
                item.filename,
                item.file_size as i64,
                item.duration_seconds,
                item.width.map(|v| v as i64),
                item.height.map(|v| v as i64),
                item.codec,
                serde_json::to_string(&item.tags)?,
                item.notes,
                item.thumbnail_path,
                item.created_at,
                item.modified_at,
                item.trim_start_seconds,
                item.trim_end_seconds
            ],
        )?;
        Ok(())
    }

    pub fn list_media(&self, kind: Option<MediaKind>) -> AppResult<Vec<MediaItem>> {
        let conn = self.conn()?;
        let sql = match kind {
            Some(_) => "SELECT * FROM media WHERE kind = ?1 ORDER BY modified_at DESC",
            None => "SELECT * FROM media ORDER BY modified_at DESC",
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = match kind {
            Some(kind) => stmt.query_map([kind.as_str()], row_to_media)?,
            None => stmt.query_map([], row_to_media)?,
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_media(&self, id: &str) -> AppResult<MediaItem> {
        let conn = self.conn()?;
        conn.query_row("SELECT * FROM media WHERE id = ?1", [id], row_to_media)
            .optional()?
            .ok_or_else(|| user_error("media item was not found"))
    }

    pub fn update_tags(&self, id: &str, tags: &[String]) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE media SET tags_json = ?1 WHERE id = ?2",
            params![serde_json::to_string(tags)?, id],
        )?;
        Ok(())
    }

    pub fn update_notes(&self, id: &str, notes: Option<&str>) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE media SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        )?;
        Ok(())
    }

    pub fn update_thumbnail(&self, id: &str, thumbnail_path: Option<&str>) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE media SET thumbnail_path = ?1 WHERE id = ?2",
            params![thumbnail_path, id],
        )?;
        Ok(())
    }

    pub fn list_collections(&self) -> AppResult<Vec<Collection>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.name, COUNT(ci.media_id) AS item_count
            FROM collections c
            LEFT JOIN collection_items ci ON ci.collection_id = c.id
            GROUP BY c.id, c.name
            ORDER BY c.name COLLATE NOCASE
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                item_count: row.get::<_, i64>(2)? as u32,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_collection_media(&self, collection_id: &str) -> AppResult<Vec<MediaItem>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT m.*
            FROM media m
            INNER JOIN collection_items ci ON ci.media_id = m.id
            WHERE ci.collection_id = ?1
            ORDER BY ci.added_at DESC
            "#,
        )?;
        let rows = stmt.query_map([collection_id], row_to_media)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_collection(&self, id: &str, name: &str, created_at: i64) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO collections (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, created_at],
        )?;
        Ok(())
    }

    pub fn add_to_collection(
        &self,
        collection_id: &str,
        media_id: &str,
        added_at: i64,
    ) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO collection_items (collection_id, media_id, added_at) VALUES (?1, ?2, ?3)",
            params![collection_id, media_id, added_at],
        )?;
        Ok(())
    }

    pub fn delete_media(&self, id: &str) -> AppResult<()> {
        let mut conn = self.conn()?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let path: Option<String> = transaction
            .query_row("SELECT path FROM media WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        transaction.execute("DELETE FROM media WHERE id = ?1", [id])?;
        if let Some(path) = path {
            let key = crate::media::normalized_path_key(std::path::Path::new(&path));
            transaction.execute("DELETE FROM probe_cache WHERE path_key = ?1", [key])?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn probe_data_is_valid(data: &ProbeData) -> bool {
    let metadata = &data.metadata;
    metadata.duration_seconds.is_finite()
        && metadata.duration_seconds >= 0.0
        && metadata
            .frame_rate
            .is_none_or(|value| value.is_finite() && value > 0.0)
        && metadata.width.is_none_or(|value| value > 0)
        && metadata.height.is_none_or(|value| value > 0)
        && data.audio_tracks.len() <= 128
}

fn row_to_media(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaItem> {
    let tags_json: String = row.get("tags_json")?;
    Ok(MediaItem {
        id: row.get("id")?,
        kind: MediaKind::from_db(row.get::<_, String>("kind")?.as_str()),
        path: row.get("path")?,
        source_path: row.get("source_path")?,
        filename: row.get("filename")?,
        file_size: row.get::<_, i64>("file_size")? as u64,
        duration_seconds: row.get("duration_seconds")?,
        width: row.get::<_, Option<i64>>("width")?.map(|v| v as u32),
        height: row.get::<_, Option<i64>>("height")?.map(|v| v as u32),
        codec: row.get("codec")?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        notes: row.get("notes")?,
        thumbnail_path: row.get("thumbnail_path")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
        trim_start_seconds: row.get("trim_start_seconds")?,
        trim_end_seconds: row.get("trim_end_seconds")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AudioTrack, MediaMetadata};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn item(path: &str, kind: MediaKind) -> MediaItem {
        MediaItem {
            id: format!("{kind:?}-{path}"),
            kind,
            path: path.to_string(),
            source_path: (kind == MediaKind::Clip).then(|| "source.mp4".to_string()),
            filename: "video.mp4".to_string(),
            file_size: 42,
            duration_seconds: 10.0,
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".to_string()),
            tags: vec!["test".to_string()],
            notes: (kind == MediaKind::Clip).then(|| "clip title".to_string()),
            thumbnail_path: None,
            created_at: 1,
            modified_at: 2,
            trim_start_seconds: (kind == MediaKind::Clip).then_some(1.0),
            trim_end_seconds: (kind == MediaKind::Clip).then_some(5.0),
        }
    }

    fn probe_data(duration_seconds: f64) -> ProbeData {
        ProbeData {
            metadata: MediaMetadata {
                duration_seconds,
                width: Some(1920),
                height: Some(1080),
                codec: Some("h264".to_string()),
                container: Some("mov,mp4".to_string()),
                frame_rate: Some(60.0),
                rotation_degrees: None,
                creation_timestamp: None,
            },
            audio_tracks: vec![AudioTrack {
                index: 1,
                codec: Some("aac".to_string()),
                channels: Some(2),
                language: Some("eng".to_string()),
                title: None,
                is_default: true,
            }],
        }
    }

    #[test]
    fn scanner_upsert_cannot_downgrade_a_clip() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelspace-db-{nonce}.sqlite"));
        let db = Database::open(path.clone()).expect("database");
        db.upsert_media(&item("C:\\clips\\protected.mp4", MediaKind::Clip))
            .expect("insert clip");
        db.upsert_media(&item("C:\\clips\\protected.mp4", MediaKind::Raw))
            .expect("scanner upsert");

        let stored = db
            .list_media(Some(MediaKind::Clip))
            .expect("list clips")
            .pop()
            .expect("protected clip");
        assert_eq!(stored.kind, MediaKind::Clip);
        assert_eq!(stored.source_path.as_deref(), Some("source.mp4"));
        assert_eq!(stored.notes.as_deref(), Some("clip title"));
        assert_eq!(stored.trim_start_seconds, Some(1.0));

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn probe_cache_requires_an_exact_fingerprint() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelspace-probe-cache-{nonce}.sqlite"));
        let db = Database::open(path.clone()).expect("database");
        let fingerprint = FileFingerprint {
            path_key: "c:\\captures\\game.mp4".to_string(),
            size: 42,
            modified_ns: 123,
        };
        db.put_probe_cache(&fingerprint, &probe_data(10.0))
            .expect("cache metadata");
        assert_eq!(
            db.get_probe_cache(&fingerprint)
                .expect("cache lookup")
                .expect("cache hit")
                .metadata
                .duration_seconds,
            10.0
        );
        let changed = FileFingerprint {
            size: 43,
            ..fingerprint
        };
        assert!(db
            .get_probe_cache(&changed)
            .expect("changed lookup")
            .is_none());
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn migrations_preserve_media_organization_data() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelspace-migration-{nonce}.sqlite"));
        {
            let db = Database::open(path.clone()).expect("database");
            let media = item("C:\\captures\\kept.mp4", MediaKind::Raw);
            db.upsert_media(&media).expect("insert media");
            db.create_collection(&"b".repeat(64), "Favorites", 1)
                .expect("collection");
            db.add_to_collection(&"b".repeat(64), &media.id, 2)
                .expect("collection item");
        }
        let conn = Connection::open(&path).expect("legacy connection");
        conn.pragma_update(None, "user_version", 1)
            .expect("legacy version");
        drop(conn);

        let reopened = Database::open(path.clone()).expect("migrated database");
        assert_eq!(reopened.list_media(None).expect("media").len(), 1);
        assert_eq!(
            reopened.list_collections().expect("collections")[0].item_count,
            1
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn deleting_media_removes_only_its_disposable_probe_cache() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("reelspace-delete-cache-{nonce}.sqlite"));
        let db = Database::open(path.clone()).expect("database");
        let media = item("C:\\captures\\delete.mp4", MediaKind::Raw);
        db.upsert_media(&media).expect("media");
        let fingerprint = FileFingerprint {
            path_key: crate::media::normalized_path_key(std::path::Path::new(&media.path)),
            size: media.file_size,
            modified_ns: 2,
        };
        db.put_probe_cache(&fingerprint, &probe_data(10.0))
            .expect("cache");
        db.delete_media(&media.id).expect("delete");
        assert_eq!(db.probe_cache_count().expect("cache count"), 0);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
