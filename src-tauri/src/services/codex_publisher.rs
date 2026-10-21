use crate::persistence::backup_sqlite;
use crate::util::{first_non_empty, text_from_value};
use chrono::Local;
use rusqlite::types::Value;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPublishPreview {
    pub active_provider: String,
    pub eligible_thread_count: usize,
    pub selected_thread_count: usize,
    pub hidden_thread_count: usize,
    pub project_count: usize,
    pub existing_catalog_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPublishResult {
    pub active_provider: String,
    pub state_thread_count: usize,
    pub catalog_thread_count: usize,
    pub hidden_thread_count: usize,
    pub purged_thread_count: usize,
    pub removed_catalog_count: usize,
    pub backup_directory: String,
    pub quarantined_rollout_count: usize,
    pub quarantine_warnings: Vec<String>,
}

pub struct CodexClientPublisher {
    pub codex_home: PathBuf,
    pub backup_root: PathBuf,
    pub state_path: PathBuf,
    pub catalog_path: PathBuf,
}

impl CodexClientPublisher {
    pub fn new(custom_codex_home: Option<&Path>, backup_root: &Path) -> Self {
        let home = if let Some(p) = custom_codex_home {
            p.to_path_buf()
        } else {
            dirs::home_dir()
                .map(|h| h.join(".codex"))
                .unwrap_or_else(|| PathBuf::from(".codex"))
        };

        let state_path = home.join("state_5.sqlite");
        let catalog_path = home.join("sqlite").join("codex-dev.db");

        Self {
            codex_home: home,
            backup_root: backup_root.to_path_buf(),
            state_path,
            catalog_path,
        }
    }

    pub fn preview(&self, selected_thread_ids: &[String]) -> Result<CodexPublishPreview, String> {
        self.require_databases()?;
        let catalog_ids = self.catalog_thread_ids().unwrap_or_default();
        let active_provider = self
            .active_provider()
            .unwrap_or_else(|_| "openai".to_string());

        let conn = Connection::open_with_flags(
            &self.state_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("无法连接 state_5: {}", e))?;

        let selected_set: HashSet<&str> = selected_thread_ids.iter().map(|s| s.as_str()).collect();
        let mut eligible_count: usize = 0;
        let mut selected_count: usize = 0;
        let mut project_paths = HashSet::new();

        for r in read_threads(&conn)?.into_iter().filter(|r| !r.is_subagent) {
            eligible_count += 1;
            if selected_set.contains(r.id.as_str()) {
                selected_count += 1;
                if let Some(cwd) = r.cwd {
                    project_paths.insert(cwd);
                }
            }
        }

        let hidden_count = eligible_count.saturating_sub(selected_count);

        Ok(CodexPublishPreview {
            active_provider,
            eligible_thread_count: eligible_count,
            selected_thread_count: selected_count,
            hidden_thread_count: hidden_count,
            project_count: project_paths.len(),
            existing_catalog_count: catalog_ids.len(),
        })
    }

    pub fn publish(
        &self,
        selected_thread_ids: &[String],
        purge_thread_ids: &[String],
    ) -> Result<CodexPublishResult, String> {
        self.require_databases()?;
        let provider = self
            .active_provider()
            .unwrap_or_else(|_| "openai".to_string());

        // Publishing hides every main thread that is not selected, so an empty or
        // unrecognized selection would hide the whole Codex sidebar.
        {
            let conn = Connection::open_with_flags(
                &self.state_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|e| format!("无法连接 state_5: {}", e))?;
            let known: HashSet<String> = read_threads(&conn)?
                .into_iter()
                .filter(|r| !r.is_subagent)
                .map(|r| r.id)
                .collect();
            if !selected_thread_ids.iter().any(|id| known.contains(id)) {
                return Err(
                    "所选会话均不在当前 Codex 索引中，未做任何修改。请先在 Codex 标签页勾选从 Codex 恢复的会话。"
                        .to_string(),
                );
            }
        }

        let backup_dir = self.backup_databases()?;

        let mut conn =
            Connection::open(&self.state_path).map_err(|e| format!("打开 state_5 失败: {}", e))?;
        // `PRAGMA busy_timeout` returns a row, which `execute_batch` rejects when
        // rusqlite's extra checks are enabled (they are, through `bundled-full`).
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;

        let catalog_path_str = self.catalog_path.to_str().ok_or("目录包含非 UTF-8 字符")?;
        conn.execute("ATTACH DATABASE ? AS client_catalog", [catalog_path_str])
            .map_err(|e| format!("关联 catalog 数据库失败: {}", e))?;

        let tx = conn
            .transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;

        // 1. Fetch all threads. Subagent threads belong to their parent conversation
        //    and are never hidden; earlier versions hid them, so restore those.
        let all_rows = read_threads(&tx)?;
        let (subagents, rows): (Vec<ThreadRow>, Vec<ThreadRow>) =
            all_rows.into_iter().partition(|r| r.is_subagent);

        for r in subagents
            .iter()
            .filter(|r| r.model_provider.as_deref() == Some(HIDDEN_PROVIDER))
        {
            tx.execute(
                "UPDATE threads SET model_provider = ?1 WHERE id = ?2",
                params![provider, r.id],
            )
            .map_err(|e| e.to_string())?;
        }

        let selected_set: HashSet<String> = selected_thread_ids.iter().cloned().collect();
        let purge_set: HashSet<String> = purge_thread_ids.iter().cloned().collect();

        let mut hidden_count = 0;
        let mut state_selected_count = 0;

        for r in &rows {
            if purge_set.contains(&r.id) {
                continue;
            }
            if selected_set.contains(&r.id) {
                state_selected_count += 1;
                tx.execute(
                    "UPDATE threads SET model_provider = ?1, archived = 0 WHERE id = ?2",
                    params![provider, r.id],
                )
                .map_err(|e| e.to_string())?;
            } else {
                hidden_count += 1;
                tx.execute(
                    "UPDATE threads SET model_provider = ?1 WHERE id = ?2",
                    params![HIDDEN_PROVIDER, r.id],
                )
                .map_err(|e| e.to_string())?;
            }
        }

        // 2. Clean purged threads from state tables
        let mut purged_count = 0;
        let mut rollout_purge_paths = Vec::new();

        for purge_id in &purge_set {
            if selected_set.contains(purge_id) {
                continue;
            }
            // query rollout path before deletion
            let mut rstmt = tx
                .prepare("SELECT rollout_path FROM threads WHERE id = ?")
                .map_err(|e| e.to_string())?;
            let rpaths: Vec<String> = rstmt
                .query_map([purge_id], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok().flatten())
                .collect();
            for rp in rpaths {
                rollout_purge_paths.push((purge_id.clone(), PathBuf::from(rp)));
            }

            let _ = tx.execute("DELETE FROM thread_spawn_edges WHERE parent_thread_id = ?1 OR child_thread_id = ?1", [purge_id]);
            let rows_del = tx
                .execute("DELETE FROM threads WHERE id = ?", [purge_id])
                .map_err(|e| e.to_string())?;
            purged_count += rows_del;
        }

        // 3. Update client_catalog.local_thread_catalog
        tx.execute(
            "CREATE TEMP TABLE context_restoration_selected_threads (thread_id TEXT PRIMARY KEY)",
            [],
        )
        .map_err(|e| e.to_string())?;

        for sid in &selected_set {
            let _ = tx.execute(
                "INSERT OR IGNORE INTO context_restoration_selected_threads VALUES(?)",
                [sid],
            );
        }

        let removed_catalog = tx
            .execute(
                "DELETE FROM client_catalog.local_thread_catalog
             WHERE host_id = 'local' AND thread_id NOT IN (
                 SELECT thread_id FROM context_restoration_selected_threads
             )",
                [],
            )
            .map_err(|e| e.to_string())?;

        // Read current observation sequence
        let mut seq_stmt = tx.prepare(
            "SELECT observation_sequence FROM client_catalog.local_thread_catalog_sync_state WHERE host_id = 'local'"
        ).map_err(|e| e.to_string())?;
        let mut sequence: i64 = seq_stmt.query_row([], |r| r.get(0)).unwrap_or(0);
        drop(seq_stmt);

        let mut catalog_inserted = 0;
        for r in &rows {
            if !selected_set.contains(&r.id) {
                continue;
            }
            sequence += 1;
            let display_title = r
                .title
                .clone()
                .or(r.first_user_message.clone())
                .unwrap_or_else(|| "Codex 会话".to_string());
            let source_kind = if r.source.as_deref() == Some("cli") {
                "cli"
            } else {
                "vscode"
            };
            let created_sec = r
                .created_at_ms
                .map(|ms| ms as f64 / 1000.0)
                .or_else(|| r.created_at.map(|s| s as f64))
                .unwrap_or(0.0);
            let updated_sec = r
                .updated_at_ms
                .map(|ms| ms as f64 / 1000.0)
                .or_else(|| r.updated_at.map(|s| s as f64))
                .unwrap_or(0.0);
            let recency_sec = r
                .recency_at_ms
                .map(|ms| ms as f64 / 1000.0)
                .or_else(|| r.recency_at.map(|s| s as f64))
                .unwrap_or(updated_sec);

            tx.execute(
                "INSERT INTO client_catalog.local_thread_catalog(
                    host_id, thread_id, display_title, source_created_at, source_updated_at,
                    cwd, source_kind, model_provider, git_branch, observation_sequence,
                    missing_candidate, thread_source, source_recency_at, pending_observed_title
                ) VALUES('local', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, 0)
                ON CONFLICT(host_id, thread_id) DO UPDATE SET
                    display_title = excluded.display_title,
                    source_created_at = excluded.source_created_at,
                    source_updated_at = excluded.source_updated_at,
                    cwd = excluded.cwd,
                    source_kind = excluded.source_kind,
                    model_provider = excluded.model_provider,
                    git_branch = excluded.git_branch,
                    observation_sequence = excluded.observation_sequence,
                    missing_candidate = 0,
                    thread_source = excluded.thread_source,
                    source_recency_at = excluded.source_recency_at,
                    pending_observed_title = 0",
                params![
                    r.id,
                    display_title,
                    created_sec,
                    updated_sec,
                    r.cwd.clone().unwrap_or_default(),
                    source_kind,
                    provider,
                    r.git_branch,
                    sequence,
                    r.thread_source
                        .clone()
                        .unwrap_or_else(|| "user".to_string()),
                    recency_sec
                ],
            )
            .map_err(|e| e.to_string())?;
            catalog_inserted += 1;
        }

        let _ = tx.execute(
            "INSERT INTO client_catalog.local_thread_catalog_hosts(host_id, host_kind)
             VALUES('local', 'local') ON CONFLICT(host_id) DO NOTHING",
            [],
        );

        tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;

        // 4. Quarantine purged rollouts
        let (quarantined_count, quarantine_warnings) =
            self.quarantine_rollouts(&rollout_purge_paths, &backup_dir);

        Ok(CodexPublishResult {
            active_provider: provider,
            state_thread_count: state_selected_count,
            catalog_thread_count: catalog_inserted,
            hidden_thread_count: hidden_count,
            purged_thread_count: purged_count,
            removed_catalog_count: removed_catalog,
            backup_directory: backup_dir.to_string_lossy().to_string(),
            quarantined_rollout_count: quarantined_count,
            quarantine_warnings,
        })
    }

    fn require_databases(&self) -> Result<(), String> {
        if !self.state_path.is_file() {
            return Err(format!("未找到 Codex state 数据库：{:?}", self.state_path));
        }
        if !self.catalog_path.is_file() {
            return Err(format!(
                "未找到 Codex catalog 数据库：{:?}",
                self.catalog_path
            ));
        }
        Ok(())
    }

    fn active_provider(&self) -> Result<String, String> {
        let conn = Connection::open_with_flags(
            &self.catalog_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT model_provider FROM local_thread_catalog
             WHERE host_id = 'local' AND model_provider IS NOT NULL
             ORDER BY source_updated_at DESC LIMIT 1",
            )
            .map_err(|e| e.to_string())?;

        let prov: Option<String> = stmt.query_row([], |r| r.get(0)).ok();
        Ok(prov.unwrap_or_else(|| "openai".to_string()))
    }

    fn catalog_thread_ids(&self) -> Result<HashSet<String>, String> {
        let conn = Connection::open_with_flags(
            &self.catalog_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT thread_id FROM local_thread_catalog WHERE host_id = 'local'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let mut res = HashSet::new();
        for r in rows.flatten() {
            res.insert(r);
        }
        Ok(res)
    }

    fn backup_databases(&self) -> Result<PathBuf, String> {
        let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let target_dir =
            self.backup_root
                .join(format!("codex_publish_{}_{}", stamp, uuid::Uuid::new_v4()));
        fs::create_dir_all(&target_dir).map_err(|e| format!("创建备份目录失败: {}", e))?;

        if self.state_path.is_file() {
            backup_sqlite(&self.state_path, &target_dir.join("state_5.sqlite"))?;
        }
        if self.catalog_path.is_file() {
            backup_sqlite(&self.catalog_path, &target_dir.join("codex-dev.db"))?;
        }

        Ok(target_dir)
    }

    fn quarantine_rollouts(
        &self,
        rollouts: &[(String, PathBuf)],
        backup_dir: &Path,
    ) -> (usize, Vec<String>) {
        if rollouts.is_empty() {
            return (0, Vec::new());
        }
        let quarantine_dir = backup_dir.join("quarantine");
        let _ = fs::create_dir_all(&quarantine_dir);

        let mut count = 0;
        let mut warnings = Vec::new();

        for (thread_id, p) in rollouts {
            if p.is_file() {
                let file_name = p.file_name().unwrap_or_default();
                let dest = quarantine_dir.join(file_name);
                if let Err(e) = fs::copy(p, &dest) {
                    warnings.push(format!("隔离备份 {} 失败: {}", thread_id, e));
                    continue;
                }
                if let Err(e) = fs::remove_file(p) {
                    warnings.push(format!("清理原始 rollout {} 失败: {}", thread_id, e));
                } else {
                    count += 1;
                }
            }
        }

        (count, warnings)
    }
}

const HIDDEN_PROVIDER: &str = "context_restoration_hidden";

struct ThreadRow {
    id: String,
    title: Option<String>,
    first_user_message: Option<String>,
    cwd: Option<String>,
    source: Option<String>,
    git_branch: Option<String>,
    thread_source: Option<String>,
    model_provider: Option<String>,
    is_subagent: bool,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    created_at_ms: Option<i64>,
    updated_at_ms: Option<i64>,
    recency_at: Option<i64>,
    recency_at_ms: Option<i64>,
}

/// Reads the Codex thread index, tolerating columns that older or newer Codex
/// builds do not have.
fn read_threads(conn: &Connection) -> Result<Vec<ThreadRow>, String> {
    let mut columns = HashSet::new();
    {
        let mut info = conn
            .prepare("PRAGMA table_info(threads)")
            .map_err(|e| e.to_string())?;
        let names = info
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?;
        for name in names.flatten() {
            columns.insert(name);
        }
    }
    if !columns.contains("id") {
        return Err("Codex state 数据库缺少 threads.id 列，无法发布".to_string());
    }
    const OPTIONAL: [&str; 14] = [
        "title",
        "name",
        "first_user_message",
        "cwd",
        "source",
        "git_branch",
        "thread_source",
        "model_provider",
        "created_at",
        "updated_at",
        "created_at_ms",
        "updated_at_ms",
        "recency_at",
        "recency_at_ms",
    ];
    let select = OPTIONAL
        .iter()
        .map(|c| {
            if columns.contains(*c) {
                c.to_string()
            } else {
                format!("NULL AS {}", c)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn
        .prepare(&format!("SELECT id, {} FROM threads", select))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let text = |idx: usize| -> rusqlite::Result<Option<String>> {
                Ok(text_from_value(&r.get::<_, Value>(idx)?))
            };
            let int = |idx: usize| -> rusqlite::Result<Option<i64>> {
                Ok(match r.get::<_, Value>(idx)? {
                    Value::Integer(i) => Some(i),
                    Value::Real(f) => Some(f as i64),
                    Value::Text(s) => s.trim().parse().ok(),
                    _ => None,
                })
            };
            let source = text(5)?;
            let thread_source = text(7)?;
            let is_subagent = thread_source.as_deref() == Some("subagent")
                || source
                    .as_deref()
                    .map(|s| s.starts_with("{\"subagent\""))
                    .unwrap_or(false);
            Ok(ThreadRow {
                id: text(0)?.unwrap_or_default(),
                title: first_non_empty([text(2)?, text(1)?]),
                first_user_message: text(3)?,
                cwd: text(4)?,
                source,
                git_branch: text(6)?,
                thread_source,
                model_provider: text(8)?,
                is_subagent,
                created_at: int(9)?,
                updated_at: int(10)?,
                created_at_ms: int(11)?,
                updated_at_ms: int(12)?,
                recency_at: int(13)?,
                recency_at_ms: int(14)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().filter(|r| !r.id.is_empty()).collect())
}
