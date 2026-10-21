use rusqlite::{Connection, DatabaseName, OpenFlags};
use std::path::Path;
use std::time::Duration;

/// Capture committed WAL transactions through SQLite instead of copying a live file.
pub fn backup_sqlite(source: &Path, destination: &Path) -> Result<(), String> {
    let connection = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Cannot open {} for backup: {}", source.display(), e))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    connection
        .backup(DatabaseName::Main, destination, None)
        .map_err(|e| format!("Cannot back up {}: {}", source.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_includes_uncheckpointed_commits() {
        let root = std::env::temp_dir().join(format!("cv-wal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.db");
        let writer = Connection::open(&source).unwrap();
        writer
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        writer
            .query_row("PRAGMA wal_autocheckpoint=0", [], |r| r.get::<_, i64>(0))
            .unwrap();
        writer
            .execute_batch(
                "CREATE TABLE messages(content TEXT); INSERT INTO messages VALUES('first');",
            )
            .unwrap();
        writer
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
        writer
            .execute("INSERT INTO messages VALUES('recent')", [])
            .unwrap();
        let destination = root.join("backup.db");
        backup_sqlite(&source, &destination).unwrap();
        let backup = Connection::open(destination).unwrap();
        let count: i64 = backup
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
