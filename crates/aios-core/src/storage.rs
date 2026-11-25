//! SQLite WAL persistence layer and versioned schema migrations for AIOS.

use crate::error::{AiosError, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const LATEST_SCHEMA_VERSION: u32 = 1;

/// Aggregated frecency row read from SQLite `frecency` table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredFrecency {
    pub hits: u32,
    pub last_used_at: i64,
    pub pinned: bool,
    pub query_prefix_hits: u32,
}

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_core_schema",
    sql: r#"
        CREATE TABLE IF NOT EXISTS frecency (
            item_id TEXT NOT NULL,
            query_prefix TEXT NOT NULL DEFAULT '',
            hits INTEGER NOT NULL DEFAULT 1,
            last_used_at INTEGER NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (item_id, query_prefix)
        );
        CREATE INDEX IF NOT EXISTS idx_frecency_item ON frecency(item_id);
        CREATE INDEX IF NOT EXISTS idx_frecency_last_used ON frecency(last_used_at DESC);

        CREATE TABLE IF NOT EXISTS clipboard_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content_hash TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            text_preview TEXT,
            blob_path TEXT,
            source_app TEXT,
            pinned INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            last_used_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_clipboard_pinned_used
            ON clipboard_entries(pinned DESC, last_used_at DESC);

        CREATE TABLE IF NOT EXISTS snippets (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            keyword TEXT UNIQUE,
            content TEXT NOT NULL,
            folder TEXT NOT NULL DEFAULT '',
            use_count INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS quicklinks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            template_url TEXT NOT NULL,
            icon TEXT,
            open_with TEXT,
            use_count INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notes (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            body_markdown TEXT NOT NULL DEFAULT '',
            pinned INTEGER NOT NULL DEFAULT 0,
            deleted_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_notes_active_updated
            ON notes(deleted_at, pinned DESC, updated_at DESC);

        CREATE TABLE IF NOT EXISTS plugin_storage (
            plugin_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value_blob BLOB NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (plugin_id, key)
        );

        CREATE TABLE IF NOT EXISTS plugin_permissions (
            plugin_id TEXT NOT NULL,
            capability TEXT NOT NULL,
            granted INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (plugin_id, capability)
        );
    "#,
}];

/// Local SQLite database handle configured with WAL mode and foreign keys.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (or creates) the SQLite database at `path`, enables WAL mode, and runs pending migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AiosError::IoPath {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.configure_pragmas(true)?;
        db.migrate()?;
        Ok(db)
    }

    /// Opens an in-memory SQLite database with all migrations applied (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.configure_pragmas(false)?;
        db.migrate()?;
        Ok(db)
    }

    fn configure_pragmas(&self, enable_wal: bool) -> Result<()> {
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.pragma_update(None, "busy_timeout", 5000_i64)?;
        if enable_wal {
            let _: String =
                self.conn
                    .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
            self.conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        Ok(())
    }

    /// Applies any unapplied schema migrations transactionally and idempotently.
    pub fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )?;

        let current = self.schema_version()?;
        for migration in MIGRATIONS {
            if migration.version <= current {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(migration.sql)
                .map_err(|source| AiosError::Migration {
                    version: migration.version,
                    name: migration.name,
                    source,
                })?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                params![migration.version, migration.name],
            )
            .map_err(|source| AiosError::Migration {
                version: migration.version,
                name: migration.name,
                source,
            })?;
            tx.commit()?;
            tracing::info!(
                version = migration.version,
                name = migration.name,
                "applied sqlite migration"
            );
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        let version: Option<u32> = self
            .conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        Ok(version.unwrap_or(0))
    }

    pub fn journal_mode(&self) -> Result<String> {
        let mode: String = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        Ok(mode)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Scoped plugin key-value read used by `host-api::storage-get`.
    pub fn plugin_storage_get(&self, plugin_id: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let value = self
            .conn
            .query_row(
                "SELECT value_blob FROM plugin_storage WHERE plugin_id = ?1 AND key = ?2",
                params![plugin_id, key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    /// Scoped plugin key-value upsert used by `host-api::storage-set`.
    pub fn plugin_storage_set(
        &self,
        plugin_id: &str,
        key: &str,
        value: &[u8],
        now_epoch: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_storage (plugin_id, key, value_blob, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(plugin_id, key) DO UPDATE SET
                value_blob = excluded.value_blob,
                updated_at = excluded.updated_at",
            params![plugin_id, key, value, now_epoch],
        )?;
        Ok(())
    }

    /// Scoped plugin key-value delete used by `host-api::storage-remove`.
    pub fn plugin_storage_remove(&self, plugin_id: &str, key: &str) -> Result<bool> {
        let affected = self.conn.execute(
            "DELETE FROM plugin_storage WHERE plugin_id = ?1 AND key = ?2",
            params![plugin_id, key],
        )?;
        Ok(affected > 0)
    }

    /// Records an item execution in the `frecency` table, updating both global hits (`''`)
    /// and query-prefix affinity when `query_prefix` is non-empty.
    pub fn record_frecency_hit(
        &self,
        item_id: &str,
        query_prefix: &str,
        now_epoch: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO frecency (item_id, query_prefix, hits, last_used_at, pinned)
             VALUES (?1, '', 1, ?2, 0)
             ON CONFLICT(item_id, query_prefix) DO UPDATE SET
                hits = frecency.hits + 1,
                last_used_at = excluded.last_used_at",
            params![item_id, now_epoch],
        )?;

        let norm_prefix = query_prefix.trim().to_lowercase();
        if !norm_prefix.is_empty() {
            let prefix_key: String = norm_prefix.chars().take(24).collect();
            self.conn.execute(
                "INSERT INTO frecency (item_id, query_prefix, hits, last_used_at, pinned)
                 VALUES (?1, ?2, 1, ?3, 0)
                 ON CONFLICT(item_id, query_prefix) DO UPDATE SET
                    hits = frecency.hits + 1,
                    last_used_at = excluded.last_used_at",
                params![item_id, prefix_key, now_epoch],
            )?;
        }
        Ok(())
    }

    /// Persists pinned state for an item on its global (`query_prefix = ''`) frecency row.
    pub fn set_item_pinned(&self, item_id: &str, pinned: bool, now_epoch: i64) -> Result<()> {
        let pin_int = i64::from(pinned);
        self.conn.execute(
            "INSERT INTO frecency (item_id, query_prefix, hits, last_used_at, pinned)
             VALUES (?1, '', 0, ?2, ?3)
             ON CONFLICT(item_id, query_prefix) DO UPDATE SET
                pinned = excluded.pinned",
            params![item_id, now_epoch, pin_int],
        )?;
        Ok(())
    }

    /// Loads all frecency records aggregated per `item_id`, including `query_prefix_hits`
    /// matching the active search prefix.
    pub fn load_frecency_records(
        &self,
        query_prefix: &str,
    ) -> Result<HashMap<String, StoredFrecency>> {
        let mut map: HashMap<String, StoredFrecency> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT item_id, hits, last_used_at, pinned
                 FROM frecency
                 WHERE query_prefix = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    StoredFrecency {
                        hits: row.get::<_, u32>(1)?,
                        last_used_at: row.get::<_, i64>(2)?,
                        pinned: row.get::<_, i64>(3)? != 0,
                        query_prefix_hits: 0,
                    },
                ))
            })?;
            for row in rows {
                let (id, rec) = row?;
                map.insert(id, rec);
            }
        }

        let norm_prefix = query_prefix.trim().to_lowercase();
        if !norm_prefix.is_empty() {
            let prefix_key: String = norm_prefix.chars().take(24).collect();
            let mut stmt = self
                .conn
                .prepare("SELECT item_id, hits FROM frecency WHERE query_prefix = ?1")?;
            let rows = stmt.query_map(params![prefix_key], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })?;
            for row in rows {
                let (id, prefix_hits) = row?;
                let entry = map.entry(id).or_default();
                entry.query_prefix_hits = prefix_hits;
            }
        }

        Ok(map)
    }

    /// Loads the set of user-hidden item IDs (e.g. hidden desktop applications).
    pub fn list_hidden_items(&self) -> Result<HashSet<String>> {
        let Some(blob) = self.plugin_storage_get("aios.applications", "hidden_items")? else {
            return Ok(HashSet::new());
        };
        let ids: Vec<String> = serde_json::from_slice(&blob).unwrap_or_default();
        Ok(ids.into_iter().collect())
    }

    /// Hides or unhides an item ID persistently.
    pub fn set_item_hidden(&self, item_id: &str, hidden: bool, now_epoch: i64) -> Result<()> {
        let mut set = self.list_hidden_items()?;
        if hidden {
            set.insert(item_id.to_owned());
        } else {
            set.remove(item_id);
        }
        let mut sorted: Vec<String> = set.into_iter().collect();
        sorted.sort();
        let bytes = serde_json::to_vec(&sorted)
            .map_err(|e| AiosError::InvalidConfig(format!("failed to encode hidden items: {e}")))?;
        self.plugin_storage_set("aios.applications", "hidden_items", &bytes, now_epoch)
    }

    /// Clears all hidden item overrides.
    pub fn clear_hidden_items(&self) -> Result<()> {
        let _ = self.plugin_storage_remove("aios.applications", "hidden_items")?;
        Ok(())
    }

    /// Reads whether a specific capability has been explicitly granted (`Some(true)`),
    /// denied (`Some(false)`), or not yet decided (`None`) for `plugin_id`.
    pub fn get_plugin_permission(&self, plugin_id: &str, capability: &str) -> Result<Option<bool>> {
        let granted: Option<i64> = self
            .conn
            .query_row(
                "SELECT granted FROM plugin_permissions WHERE plugin_id = ?1 AND capability = ?2",
                params![plugin_id, capability],
                |row| row.get(0),
            )
            .optional()?;
        Ok(granted.map(|v| v != 0))
    }

    /// Persists a user capability decision (`granted = true | false`) for `plugin_id`.
    pub fn set_plugin_permission(
        &self,
        plugin_id: &str,
        capability: &str,
        granted: bool,
        now_epoch: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, capability, granted, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(plugin_id, capability) DO UPDATE SET
                granted = excluded.granted,
                updated_at = excluded.updated_at",
            params![plugin_id, capability, i64::from(granted), now_epoch],
        )?;
        Ok(())
    }

    /// Lists all persisted capability grants for `plugin_id`.
    pub fn list_plugin_permissions(&self, plugin_id: &str) -> Result<HashMap<String, bool>> {
        let mut stmt = self
            .conn
            .prepare("SELECT capability, granted FROM plugin_permissions WHERE plugin_id = ?1")?;
        let rows = stmt.query_map(params![plugin_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (cap, granted) = row?;
            map.insert(cap, granted);
        }
        Ok(map)
    }

    /// Deletes all persisted permission grants and scoped KV data for an uninstalled plugin.
    pub fn purge_plugin_state(&self, plugin_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plugin_permissions WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        let pref_ns = format!("__pref__:{plugin_id}");
        let cache_ns = format!("__cache__:{plugin_id}");
        let secret_ns = format!("__secret__:{plugin_id}");
        self.conn.execute(
            "DELETE FROM plugin_storage WHERE plugin_id IN (?1, ?2, ?3, ?4)",
            params![plugin_id, pref_ns, cache_ns, secret_ns],
        )?;
        Ok(())
    }

    /// Reads a persisted plugin preference value.
    pub fn get_plugin_preference(&self, plugin_id: &str, name: &str) -> Result<Option<String>> {
        let ns = format!("__pref__:{plugin_id}");
        let Some(bytes) = self.plugin_storage_get(&ns, name)? else {
            return Ok(None);
        };
        Ok(String::from_utf8(bytes).ok())
    }

    /// Writes a persisted plugin preference value.
    pub fn set_plugin_preference(
        &self,
        plugin_id: &str,
        name: &str,
        value: &str,
        now_epoch: i64,
    ) -> Result<()> {
        let ns = format!("__pref__:{plugin_id}");
        self.plugin_storage_set(&ns, name, value.as_bytes(), now_epoch)
    }

    /// Reads a TTL cache entry for `plugin_id`, automatically expiring entries past `now_epoch`.
    pub fn plugin_cache_get(
        &self,
        plugin_id: &str,
        key: &str,
        now_epoch: i64,
    ) -> Result<Option<Vec<u8>>> {
        let ns = format!("__cache__:{plugin_id}");
        let Some(blob) = self.plugin_storage_get(&ns, key)? else {
            return Ok(None);
        };
        if blob.len() < 8 {
            return Ok(None);
        }
        let mut ts_bytes = [0u8; 8];
        ts_bytes.copy_from_slice(&blob[0..8]);
        let expires_at = i64::from_be_bytes(ts_bytes);
        if expires_at > 0 && now_epoch >= expires_at {
            let _ = self.plugin_storage_remove(&ns, key);
            return Ok(None);
        }
        Ok(Some(blob[8..].to_vec()))
    }

    /// Stores a TTL cache entry for `plugin_id` (`ttl_seconds == 0` means no expiry).
    pub fn plugin_cache_set(
        &self,
        plugin_id: &str,
        key: &str,
        value: &[u8],
        ttl_seconds: u64,
        now_epoch: i64,
    ) -> Result<()> {
        let ns = format!("__cache__:{plugin_id}");
        let expires_at = if ttl_seconds == 0 {
            0
        } else {
            now_epoch.saturating_add(i64::try_from(ttl_seconds).unwrap_or(i64::MAX))
        };
        let mut blob = Vec::with_capacity(8 + value.len());
        blob.extend_from_slice(&expires_at.to_be_bytes());
        blob.extend_from_slice(value);
        self.plugin_storage_set(&ns, key, &blob, now_epoch)
    }

    /// Local fallback secret storage scoped per plugin ID (used when Secret Service D-Bus is unavailable).
    pub fn plugin_secret_get(&self, plugin_id: &str, key: &str) -> Result<Option<String>> {
        let ns = format!("__secret__:{plugin_id}");
        let Some(bytes) = self.plugin_storage_get(&ns, key)? else {
            return Ok(None);
        };
        Ok(String::from_utf8(bytes).ok())
    }

    /// Writes a local plugin secret scoped per plugin ID.
    pub fn plugin_secret_set(
        &self,
        plugin_id: &str,
        key: &str,
        val: &str,
        now_epoch: i64,
    ) -> Result<()> {
        let ns = format!("__secret__:{plugin_id}");
        self.plugin_storage_set(&ns, key, val.as_bytes(), now_epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const EXPECTED_TABLES: &[&str] = &[
        "schema_migrations",
        "frecency",
        "clipboard_entries",
        "snippets",
        "quicklinks",
        "notes",
        "plugin_storage",
        "plugin_permissions",
    ];

    #[test]
    fn on_disk_database_enables_wal_and_applies_migrations_idempotently() {
        let tmp = TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("data").join("aios.db");

        let mut db = Database::open(&db_path).expect("open sqlite db");
        assert_eq!(
            db.journal_mode().expect("journal mode").to_lowercase(),
            "wal"
        );
        assert_eq!(
            db.schema_version().expect("schema version"),
            LATEST_SCHEMA_VERSION
        );

        // Re-running migrate and reopening must be completely idempotent.
        db.migrate().expect("idempotent migrate");
        drop(db);

        let reopened = Database::open(&db_path).expect("reopen sqlite db");
        assert_eq!(
            reopened.schema_version().expect("reopened version"),
            LATEST_SCHEMA_VERSION
        );

        for table in EXPECTED_TABLES {
            let exists: bool = reopened
                .connection()
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .expect("check table existence");
            assert!(exists, "expected table {table} to exist");
        }
    }

    #[test]
    fn plugin_kv_storage_round_trip_and_isolation() {
        let db = Database::open_in_memory().expect("in-memory db");

        db.plugin_storage_set("com.example.a", "token", b"alpha", 1_700_000_000)
            .expect("set plugin a");
        db.plugin_storage_set("com.example.b", "token", b"beta", 1_700_000_001)
            .expect("set plugin b");

        assert_eq!(
            db.plugin_storage_get("com.example.a", "token")
                .expect("get a"),
            Some(b"alpha".to_vec())
        );
        assert_eq!(
            db.plugin_storage_get("com.example.b", "token")
                .expect("get b"),
            Some(b"beta".to_vec())
        );

        db.plugin_storage_set("com.example.a", "token", b"alpha-v2", 1_700_000_002)
            .expect("upsert plugin a");
        assert_eq!(
            db.plugin_storage_get("com.example.a", "token")
                .expect("get a updated"),
            Some(b"alpha-v2".to_vec())
        );

        assert!(
            db.plugin_storage_remove("com.example.a", "token")
                .expect("remove a")
        );
        assert_eq!(
            db.plugin_storage_get("com.example.a", "token")
                .expect("get a deleted"),
            None
        );
        assert_eq!(
            db.plugin_storage_get("com.example.b", "token")
                .expect("b unaffected"),
            Some(b"beta".to_vec())
        );
    }

    #[test]
    fn core_tables_accept_valid_records() {
        let db = Database::open_in_memory().expect("in-memory db");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO frecency (item_id, query_prefix, hits, last_used_at, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["app:firefox", "ff", 5, 1_700_000_100, 1],
        )
        .expect("insert frecency");

        conn.execute(
            "INSERT INTO clipboard_entries (content_hash, kind, mime_type, text_preview, source_app, created_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params!["hash123", "text", "text/plain", "hello wayland", "alacritty", 1_700_000_100, 1_700_000_100],
        )
        .expect("insert clipboard");

        conn.execute(
            "INSERT INTO snippets (id, name, keyword, content, folder, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "sn1",
                "Work Email",
                ";mail",
                "me@example.org",
                "Personal",
                1_700_000_100,
                1_700_000_100
            ],
        )
        .expect("insert snippet");

        conn.execute(
            "INSERT INTO quicklinks (id, name, template_url, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "ql1",
                "Rust Docs",
                "https://docs.rs/{query}",
                1_700_000_100,
                1_700_000_100
            ],
        )
        .expect("insert quicklink");

        conn.execute(
            "INSERT INTO notes (id, title, body_markdown, pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["n1", "Scratch", "# Notes", 0, 1_700_000_100, 1_700_000_100],
        )
        .expect("insert note");

        conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, capability, granted, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params!["com.example.docker", "clipboard_write", 1, 1_700_000_100],
        )
        .expect("insert permission");
    }

    #[test]
    fn frecency_hits_pinning_and_query_prefix_round_trip() {
        let db = Database::open_in_memory().expect("in-memory db");

        db.record_frecency_hit("app:firefox", "ff", 1_700_000_100)
            .expect("first hit");
        db.record_frecency_hit("app:firefox", "  FF  ", 1_700_000_200)
            .expect("second hit normalized prefix");
        db.record_frecency_hit("app:firefox", "", 1_700_000_300)
            .expect("third global hit");
        db.set_item_pinned("app:firefox", true, 1_700_000_350)
            .expect("pin item");
        db.set_item_pinned("app:alacritty", true, 1_700_000_360)
            .expect("pin item without hits");

        let with_prefix = db
            .load_frecency_records("ff")
            .expect("load frecency with prefix");
        let ff = with_prefix.get("app:firefox").expect("firefox row");
        assert_eq!(ff.hits, 3);
        assert_eq!(ff.last_used_at, 1_700_000_300);
        assert!(ff.pinned);
        assert_eq!(ff.query_prefix_hits, 2);

        let term = with_prefix.get("app:alacritty").expect("alacritty row");
        assert_eq!(term.hits, 0);
        assert!(term.pinned);
        assert_eq!(term.query_prefix_hits, 0);

        let no_prefix = db
            .load_frecency_records("")
            .expect("load frecency without prefix");
        assert_eq!(
            no_prefix
                .get("app:firefox")
                .expect("firefox row")
                .query_prefix_hits,
            0
        );

        db.set_item_pinned("app:firefox", false, 1_700_000_400)
            .expect("unpin item");
        let after_unpin = db.load_frecency_records("ff").expect("reload");
        assert!(!after_unpin.get("app:firefox").expect("firefox").pinned);
    }

    #[test]
    fn hidden_items_persistence_and_clear() {
        let db = Database::open_in_memory().expect("in-memory db");
        assert!(db.list_hidden_items().expect("initial empty").is_empty());

        db.set_item_hidden("app:b", true, 1_700_000_100)
            .expect("hide b");
        db.set_item_hidden("app:a", true, 1_700_000_101)
            .expect("hide a");
        let hidden = db.list_hidden_items().expect("list hidden");
        assert_eq!(hidden.len(), 2);
        assert!(hidden.contains("app:a"));
        assert!(hidden.contains("app:b"));

        db.set_item_hidden("app:a", false, 1_700_000_102)
            .expect("unhide a");
        let remaining = db.list_hidden_items().expect("list remaining");
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains("app:b"));

        db.clear_hidden_items().expect("clear hidden");
        assert!(db.list_hidden_items().expect("cleared").is_empty());
    }
}
