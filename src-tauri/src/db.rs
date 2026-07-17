use crate::error::{user_error, AppResult};
use crate::models::{AppSettings, Collection, MediaItem, MediaKind};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn conn(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| user_error("database connection was poisoned"))
    }

    fn migrate(&self) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

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
            "#,
        )?;
        Ok(())
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
              kind = excluded.kind,
              source_path = excluded.source_path,
              filename = excluded.filename,
              file_size = excluded.file_size,
              duration_seconds = excluded.duration_seconds,
              width = excluded.width,
              height = excluded.height,
              codec = excluded.codec,
              tags_json = CASE WHEN media.tags_json = '[]' THEN excluded.tags_json ELSE media.tags_json END,
              thumbnail_path = COALESCE(excluded.thumbnail_path, media.thumbnail_path),
              modified_at = excluded.modified_at,
              trim_start_seconds = excluded.trim_start_seconds,
              trim_end_seconds = excluded.trim_end_seconds
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
        let conn = self.conn()?;
        conn.execute("DELETE FROM media WHERE id = ?1", [id])?;
        Ok(())
    }
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
