use chrono::Local;
use rusqlite::{Connection, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 9;

const FOLDER_PINS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS folder_pins (
    source TEXT NOT NULL CHECK (source IN ('codex', 'antigravity', 'claude')),
    path TEXT NOT NULL,
    pinned_at TEXT NOT NULL,
    PRIMARY KEY (source, path)
);";

pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('openai_responses', 'openai_compatible_chat')),
    base_url TEXT NOT NULL,
    default_model TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    request_timeout_s REAL NOT NULL DEFAULT 120,
    max_input_tokens INTEGER,
    api_key_ciphertext BLOB NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    source_title TEXT,
    title_user_edited INTEGER NOT NULL DEFAULT 0 CHECK (title_user_edited IN (0, 1)),
    system_prompt TEXT NOT NULL DEFAULT '',
    active_provider_id TEXT REFERENCES providers(id) ON DELETE SET NULL,
    active_model TEXT,
    active_leaf_message_id TEXT,
    project_path TEXT,
    source_kind TEXT NOT NULL DEFAULT 'general',
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    pinned_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
    sequence_no INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('system', 'developer', 'user', 'assistant')),
    content TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('complete', 'streaming', 'interrupted', 'failed', 'cancelled')
    ),
    provider_id TEXT REFERENCES providers(id) ON DELETE SET NULL,
    model TEXT,
    remote_response_id TEXT,
    source_type TEXT NOT NULL DEFAULT 'native',
    source_external_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (conversation_id, sequence_no),
    UNIQUE (source_type, source_external_id)
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation_seq
    ON messages(conversation_id, sequence_no);
CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_message_id);

CREATE TABLE IF NOT EXISTS request_attempts (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_message_id TEXT NOT NULL REFERENCES messages(id),
    assistant_message_id TEXT NOT NULL REFERENCES messages(id),
    provider_id TEXT NOT NULL REFERENCES providers(id),
    model TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    error_category TEXT,
    error_message TEXT,
    remote_request_id TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS context_checkpoints (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    through_message_id TEXT NOT NULL REFERENCES messages(id),
    summary_version INTEGER NOT NULL,
    summary_text TEXT NOT NULL,
    created_by_provider TEXT REFERENCES providers(id) ON DELETE SET NULL,
    created_by_model TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (conversation_id, summary_version)
);

CREATE TABLE IF NOT EXISTS import_jobs (
    id TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_sha256 TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL,
    conversation_count INTEGER NOT NULL DEFAULT 0,
    message_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    error_report_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS privacy_confirmations (
    host TEXT PRIMARY KEY,
    confirmed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recovery_exclusions (
    source_type TEXT NOT NULL,
    source_conversation_id TEXT NOT NULL,
    title TEXT NOT NULL,
    project_path TEXT,
    excluded_at TEXT NOT NULL,
    PRIMARY KEY (source_type, source_conversation_id)
);

"#;

/// Full-text index over message content. The trigram tokenizer matches substrings,
/// which Chinese text needs because it has no spaces between words. The index reads
/// content from `messages` by rowid, so the database must not be VACUUMed without
/// rebuilding it.
pub const FTS_SCHEMA: &str = r#"
DROP TRIGGER IF EXISTS messages_ai;
DROP TRIGGER IF EXISTS messages_au;
DROP TRIGGER IF EXISTS messages_ad;
DROP TRIGGER IF EXISTS conversations_title_au;
DROP TRIGGER IF EXISTS message_fts_ai;
DROP TRIGGER IF EXISTS message_fts_au;
DROP TRIGGER IF EXISTS message_fts_ad;
DROP TABLE IF EXISTS message_fts;

CREATE VIRTUAL TABLE message_fts USING fts5(
    content, content = 'messages', content_rowid = 'rowid', tokenize = 'trigram'
);

CREATE TRIGGER message_fts_ai AFTER INSERT ON messages BEGIN
    INSERT INTO message_fts(rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER message_fts_ad AFTER DELETE ON messages BEGIN
    INSERT INTO message_fts(message_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;

CREATE TRIGGER message_fts_au AFTER UPDATE OF content ON messages BEGIN
    INSERT INTO message_fts(message_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO message_fts(rowid, content) VALUES (new.rowid, new.content);
END;

INSERT INTO message_fts(message_fts) VALUES ('rebuild');
"#;

pub struct Database {
    path: PathBuf,
    backups_dir: PathBuf,
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &Path, backups_dir: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::create_dir_all(backups_dir);

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;

        let db = Self {
            path: path.to_path_buf(),
            backups_dir: backups_dir.to_path_buf(),
            conn: Mutex::new(conn),
        };

        db.initialize()?;
        Ok(db)
    }

    /// A panic while the lock was held must not make the database unusable for the
    /// rest of the session; SQLite itself keeps the data consistent.
    pub fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare("PRAGMA user_version")?;
        let version: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(version)
    }

    pub fn initialize(&self) -> Result<i64> {
        let conn = self.lock_conn();
        let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if current_version == SCHEMA_VERSION {
            return Ok(current_version);
        }

        if current_version > SCHEMA_VERSION {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some(format!(
                    "数据库版本 {} 高于应用支持的 {}",
                    current_version, SCHEMA_VERSION
                )),
            ));
        }

        if current_version > 0 {
            // Keep a copy of the data as it was before the schema changes.
            let destination = self.new_backup_path();
            conn.backup(rusqlite::DatabaseName::Main, &destination, None)?;
        }

        let mut version = current_version;
        let tx = conn.unchecked_transaction()?;

        if version == 0 {
            tx.execute_batch(SCHEMA_V1)?;
            tx.execute_batch(FOLDER_PINS_SCHEMA)?;
            tx.execute_batch(FTS_SCHEMA)?;
            Self::add_column_if_missing(&tx, "conversations", "project_path", "TEXT")?;
            Self::add_column_if_missing(
                &tx,
                "conversations",
                "source_kind",
                "TEXT NOT NULL DEFAULT 'general'",
            )?;
            Self::add_column_if_missing(&tx, "conversations", "source_title", "TEXT")?;
            Self::add_column_if_missing(&tx, "conversations", "pinned_at", "TEXT")?;
            Self::add_column_if_missing(
                &tx,
                "conversations",
                "title_user_edited",
                "INTEGER NOT NULL DEFAULT 0 CHECK (title_user_edited IN (0, 1))",
            )?;
            version = SCHEMA_VERSION;
            tx.execute(&format!("PRAGMA user_version = {}", version), [])?;
        } else {
            if version < 2 {
                Self::add_column_if_missing(&tx, "conversations", "project_path", "TEXT")?;
                version = 2;
            }
            if version < 3 {
                Self::add_column_if_missing(
                    &tx,
                    "conversations",
                    "source_kind",
                    "TEXT NOT NULL DEFAULT 'general'",
                )?;
                version = 3;
            }
            if version < 4 {
                tx.execute(
                    "CREATE TABLE IF NOT EXISTS recovery_exclusions (
                        source_type TEXT NOT NULL,
                        source_conversation_id TEXT NOT NULL,
                        title TEXT NOT NULL,
                        project_path TEXT,
                        excluded_at TEXT NOT NULL,
                        PRIMARY KEY (source_type, source_conversation_id)
                    )",
                    [],
                )?;
                version = 4;
            }
            if version < 5 {
                version = 5;
            }
            if version < 6 {
                Self::add_column_if_missing(&tx, "conversations", "source_title", "TEXT")?;
                Self::add_column_if_missing(
                    &tx,
                    "conversations",
                    "title_user_edited",
                    "INTEGER NOT NULL DEFAULT 0 CHECK (title_user_edited IN (0, 1))",
                )?;
                version = 6;
            }
            if version < 7 {
                tx.execute_batch(FTS_SCHEMA)?;
                // Earlier versions stored the import time; use the message times instead.
                tx.execute_batch(
                    "UPDATE conversations SET
                        created_at = COALESCE((
                            SELECT MIN(m.created_at) FROM messages m
                            WHERE m.conversation_id = conversations.id
                              AND m.created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*'
                        ), created_at),
                        updated_at = COALESCE((
                            SELECT MAX(m.created_at) FROM messages m
                            WHERE m.conversation_id = conversations.id
                              AND m.created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*'
                        ), updated_at);",
                )?;
                version = 7;
            }
            if version < 8 {
                Self::add_column_if_missing(&tx, "conversations", "pinned_at", "TEXT")?;
                version = 8;
            }
            if version < 9 {
                tx.execute_batch(FOLDER_PINS_SCHEMA)?;
                version = 9;
            }
            tx.execute(&format!("PRAGMA user_version = {}", version), [])?;
        }

        tx.commit()?;
        Ok(current_version)
    }

    fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        col_type: &str,
    ) -> Result<()> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !columns.iter().any(|c| c == column) {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, col_type),
                [],
            )?;
        }
        Ok(())
    }

    fn new_backup_path(&self) -> PathBuf {
        let _ = fs::create_dir_all(&self.backups_dir);
        let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        self.backups_dir.join(format!("conversations-{}.db", stamp))
    }

    pub fn backup(&self, dest: Option<&Path>) -> Result<PathBuf> {
        let destination = dest
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.new_backup_path());

        if let Some(p) = destination.parent() {
            let _ = fs::create_dir_all(p);
        }

        let conn = self.lock_conn();
        conn.backup(rusqlite::DatabaseName::Main, &destination, None)?;
        Ok(destination)
    }

    pub fn backup_if_needed(&self) -> Result<Option<PathBuf>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let today_prefix = format!("conversations-{}-", Local::now().format("%Y%m%d"));
        if let Ok(entries) = fs::read_dir(&self.backups_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&today_prefix) && name.ends_with(".db") {
                    return Ok(None);
                }
            }
        }

        let dest = self.backup(None)?;
        Ok(Some(dest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_init() {
        let temp_dir = std::env::temp_dir().join("test_db_init_vault");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let backups = temp_dir.join("backups");

        let db = Database::new(&db_path, &backups).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn test_full_startup_flow() {
        use std::sync::Arc;
        let temp_dir = std::env::temp_dir().join("test_db_startup_vault");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("conversations.db");
        let backups = temp_dir.join("backups");

        let db = Arc::new(Database::new(&db_path, &backups).unwrap());
        let repo = crate::persistence::Repository::new(&db);
        let _ = repo.recover_stale_requests();
        let _ = db.backup_if_needed();

        let list = repo.list_conversations(false, None).unwrap();
        assert_eq!(list.len(), 0);
    }
}
