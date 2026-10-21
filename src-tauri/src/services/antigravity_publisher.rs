use chrono::{Local, Utc};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::MessageRole;
use crate::persistence::{backup_sqlite, Database, Repository};
use crate::services::AntigravityClientController;
use crate::util::percent_encode_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityPublishCandidate {
    pub conversation_id: String, // Context Vault ID
    pub antigravity_id: String,  // Antigravity UUID
    pub title: String,
    pub project_path: Option<String>,
    pub message_count: usize,
    pub exists_in_target: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityPublishPreview {
    pub candidates: Vec<AntigravityPublishCandidate>,
    pub total_count: usize,
    pub new_count: usize, // In Vault but NOT in current Antigravity DB (e.g. from other account)
    pub existing_count: usize, // Already in current Antigravity DB
    pub target_db_path: String,
    pub is_antigravity_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityPublishResult {
    pub published_count: usize,
    pub new_count: usize,
    pub updated_count: usize,
    pub backup_path: Option<String>,
    pub warnings: Vec<String>,
}

pub struct AntigravityPublisher<'a> {
    db: &'a Database,
    antigravity_home: PathBuf,
    backup_root: PathBuf,
}

impl<'a> AntigravityPublisher<'a> {
    pub fn new(db: &'a Database, custom_home: Option<&Path>, backup_root: &Path) -> Self {
        let home = if let Some(p) = custom_home {
            p.to_path_buf()
        } else {
            let portable = PathBuf::from("./data/antigravity");
            if portable.is_dir() {
                portable
            } else if let Ok(env_home) = std::env::var("ANTIGRAVITY_HOME") {
                PathBuf::from(env_home)
            } else {
                dirs::home_dir()
                    .map(|h| h.join(".gemini").join("antigravity"))
                    .unwrap_or_else(|| PathBuf::from(".gemini").join("antigravity"))
            }
        };

        Self {
            db,
            antigravity_home: home,
            backup_root: backup_root.to_path_buf(),
        }
    }

    /// 检查 Antigravity 是否正在运行
    pub fn is_antigravity_running() -> bool {
        AntigravityClientController::new().is_running()
    }

    /// 从消息关联或会话 ID 获取对应的 Antigravity UUID
    fn resolve_antigravity_id(conn: &Connection, conversation_id: &str) -> String {
        let stmt = conn
            .prepare(
                "SELECT source_external_id FROM messages
                 WHERE conversation_id = ?1
                   AND source_type = 'antigravity_transcript'
                   AND source_external_id IS NOT NULL
                 LIMIT 1",
            )
            .ok();

        if let Some(mut s) = stmt {
            let ext_res = s.query_row(params![conversation_id], |row| row.get::<_, String>(0));
            if let Ok(ext_id) = ext_res {
                // 格式：antigravity_transcript:<uuid>:user:0
                let parts: Vec<&str> = ext_id.split(':').collect();
                if parts.len() >= 3 && !parts[1].trim().is_empty() {
                    return parts[1].trim().to_string();
                }
            }
        }

        conversation_id.to_string()
    }

    /// 编码路径为 Antigravity workspace_uris JSON 格式
    /// Writes the same form Antigravity uses: `["file:///g%3A/dir/sub%20dir"]`.
    pub fn encode_workspace_uri(path: Option<&str>) -> String {
        if let Some(p) = path {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                let mut normalized = trimmed.replace('\\', "/");
                let bytes = normalized.as_bytes();
                if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
                    normalized = format!("{}{}", normalized[..1].to_lowercase(), &normalized[1..]);
                }
                let uri = format!(
                    "file:///{}",
                    percent_encode_path(normalized.trim_start_matches('/'))
                );
                return serde_json::to_string(&vec![uri]).unwrap_or_else(|_| "[]".to_string());
            }
        }
        "[]".to_string()
    }

    /// 获取当前目标 Antigravity 数据库中已存在的所有会话 ID
    fn get_existing_target_ids(&self) -> HashSet<String> {
        let summaries_path = self.antigravity_home.join("conversation_summaries.db");
        if !summaries_path.is_file() {
            return HashSet::new();
        }

        let conn = match Connection::open_with_flags(
            &summaries_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        ) {
            Ok(c) => c,
            Err(_) => return HashSet::new(),
        };

        let mut set = HashSet::new();
        if let Ok(mut stmt) = conn.prepare("SELECT conversation_id FROM conversation_summaries") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for id in rows.flatten() {
                    set.insert(id);
                }
            }
        }
        set
    }

    /// 预览将要同步/发布至 Antigravity 的会话列表
    pub fn preview(&self, selected_ids: &[String]) -> Result<AntigravityPublishPreview, String> {
        let is_running = Self::is_antigravity_running();
        let conn = self.db.lock_conn();
        let target_existing = self.get_existing_target_ids();

        let query = if selected_ids.is_empty() {
            "SELECT id, title, project_path, updated_at FROM conversations WHERE source_kind = 'antigravity' ORDER BY updated_at DESC".to_string()
        } else {
            let placeholders = selected_ids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "SELECT id, title, project_path, updated_at FROM conversations WHERE id IN ({}) ORDER BY updated_at DESC",
                placeholders
            )
        };

        let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;

        let fetched_rows: Vec<(String, String, Option<String>, String)> = if selected_ids.is_empty()
        {
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.flatten().collect()
        } else {
            let params_vec: Vec<&dyn rusqlite::ToSql> = selected_ids
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            let rows = stmt
                .query_map(params_vec.as_slice(), |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.flatten().collect()
        };

        let mut candidates = Vec::new();
        let mut new_count = 0;
        let mut existing_count = 0;

        for (conv_id, title, project_path, updated_at) in fetched_rows {
            let ag_id = Self::resolve_antigravity_id(&conn, &conv_id);
            let exists = target_existing.contains(&ag_id);

            let msg_count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
                    params![&conv_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if exists {
                existing_count += 1;
            } else {
                new_count += 1;
            }

            candidates.push(AntigravityPublishCandidate {
                conversation_id: conv_id,
                antigravity_id: ag_id,
                title,
                project_path,
                message_count: msg_count,
                exists_in_target: exists,
                updated_at,
            });
        }

        let total_count = candidates.len();
        let target_db_path = self
            .antigravity_home
            .join("conversation_summaries.db")
            .to_string_lossy()
            .to_string();

        Ok(AntigravityPublishPreview {
            candidates,
            total_count,
            new_count,
            existing_count,
            target_db_path,
            is_antigravity_running: is_running,
        })
    }

    /// 执行同步/发布会话至 Antigravity
    pub fn publish(&self, selected_ids: &[String]) -> Result<AntigravityPublishResult, String> {
        let repo = Repository::new(self.db);

        if selected_ids.is_empty() {
            return Err("未选择任何要更新至 Antigravity 的会话".to_string());
        }

        // 1. 确保目标目录结构就绪
        fs::create_dir_all(&self.antigravity_home).map_err(|e| {
            format!(
                "无法创建 Antigravity 目录 {}: {}",
                self.antigravity_home.display(),
                e
            )
        })?;
        let brain_dir = self.antigravity_home.join("brain");
        fs::create_dir_all(&brain_dir).map_err(|e| {
            format!(
                "无法创建 Antigravity brain 目录 {}: {}",
                brain_dir.display(),
                e
            )
        })?;

        let summaries_db_path = self.antigravity_home.join("conversation_summaries.db");

        // 2. 安全备份目标数据库
        let mut backup_path_str = None;
        if summaries_db_path.is_file() {
            let backup_dir = self.backup_root.join("antigravity");
            fs::create_dir_all(&backup_dir).map_err(|e| format!("创建备份目录失败: {}", e))?;
            let timestamp = Local::now().format("%Y%m%d_%H%M%S");
            let backup_file = backup_dir.join(format!(
                "conversation_summaries_backup_{}_{}.db",
                timestamp,
                uuid::Uuid::new_v4()
            ));
            backup_sqlite(&summaries_db_path, &backup_file)?;
            backup_path_str = Some(backup_file.to_string_lossy().to_string());
        }

        // 3. 打开目标数据库（若不存在则自动初始化）
        let mut target_conn = Connection::open(&summaries_db_path).map_err(|e| {
            format!(
                "无法打开或创建 Antigravity 索引数据库 {}: {}",
                summaries_db_path.display(),
                e
            )
        })?;

        // 开启 WAL 模式以支持并发访问
        let _ = target_conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0));

        // 确保目标表结构存在
        target_conn
            .execute(
                "CREATE TABLE IF NOT EXISTS conversation_summaries (
                    conversation_id text PRIMARY KEY,
                    title text NOT NULL DEFAULT '',
                    preview text NOT NULL DEFAULT '',
                    step_count integer NOT NULL DEFAULT 0,
                    last_modified_time datetime NOT NULL,
                    workspace_uris text NOT NULL,
                    status text NOT NULL DEFAULT '',
                    source text NOT NULL DEFAULT '',
                    project_id text NOT NULL DEFAULT '',
                    agent_name text NOT NULL DEFAULT '',
                    parent_conversation_id text NOT NULL DEFAULT '',
                    nesting_depth integer NOT NULL DEFAULT 0,
                    battle_id text NOT NULL DEFAULT '',
                    winning_conversation_id text NOT NULL DEFAULT '',
                    not_fully_idle numeric NOT NULL DEFAULT false,
                    killed numeric NOT NULL DEFAULT false,
                    last_user_input_time datetime NOT NULL,
                    last_user_input_step_index integer NOT NULL DEFAULT -1,
                    app_data_dir text NOT NULL DEFAULT '',
                    raw_summary blob,
                    group_id text NOT NULL DEFAULT ''
                )",
                [],
            )
            .map_err(|e| format!("初始化 Antigravity 表结构失败: {}", e))?;

        let mut existing_set = HashSet::new();
        if let Ok(mut stmt) =
            target_conn.prepare("SELECT conversation_id FROM conversation_summaries")
        {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for id in rows.flatten() {
                    existing_set.insert(id);
                }
            }
        }
        let mut published_count = 0;
        let mut new_count = 0;
        let mut updated_count = 0;
        let mut warnings = Vec::new();

        // Antigravity stores UTC times as `2026-02-02 08:54:59.6892244+00:00`.
        let now_str = Utc::now()
            .format("%Y-%m-%d %H:%M:%S.%f")
            .to_string()
            .chars()
            .take(27)
            .collect::<String>()
            + "+00:00";
        let _ = target_conn.busy_timeout(std::time::Duration::from_secs(10));
        let target_tx = target_conn
            .transaction()
            .map_err(|e| format!("开启 Antigravity 数据库事务失败: {}", e))?;

        // 4. 预先读取所选会话的元数据（在独立作用域中释放数据库锁，避免与 load_active_path 死锁）
        let candidate_meta: Vec<(String, String, String, Option<String>)> = {
            let conn = self.db.lock_conn();
            let mut list = Vec::new();
            for conv_id in selected_ids {
                let ag_id = Self::resolve_antigravity_id(&conn, conv_id);
                let conv_res = conn.query_row(
                    "SELECT title, project_path FROM conversations WHERE id = ?1",
                    params![conv_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
                );
                match conv_res {
                    Ok((title, project_path)) => {
                        list.push((conv_id.clone(), ag_id, title, project_path));
                    }
                    Err(e) => {
                        warnings.push(format!("会话 {} 未在本地找到: {}", conv_id, e));
                    }
                }
            }
            list
        };

        for (conv_id, ag_id, title, project_path) in candidate_meta {
            let exists_in_target = existing_set.contains(&ag_id);

            // 获取该会话全部消息（内部会安全获取 self.db.lock_conn()）
            let messages = repo.load_active_path(&conv_id).unwrap_or_default();
            let step_count = (messages.len() as i64 * 2).max(2);

            // 在 Antigravity 中，title 与 preview 均作为会话展示标题使用
            let preview = title.clone();
            let workspace_uris = Self::encode_workspace_uri(project_path.as_deref());

            // 5. 重建/确保 brain/<ag_id>/.system_generated/logs/transcript.jsonl
            let logs_dir = brain_dir
                .join(&ag_id)
                .join(".system_generated")
                .join("logs");
            let _ = fs::create_dir_all(&logs_dir);

            let transcript_file = logs_dir.join("transcript.jsonl");
            let transcript_full_file = logs_dir.join("transcript_full.jsonl");

            // 若 transcript 文件不存在或为空，则从本地会话库写出标准 JSONL
            let should_write_transcript = !transcript_file.is_file()
                || fs::metadata(&transcript_file)
                    .map(|m| m.len() == 0)
                    .unwrap_or(true);

            if should_write_transcript && !messages.is_empty() {
                let mut jsonl_lines = Vec::new();
                for (idx, msg) in messages.iter().enumerate() {
                    let source = if msg.role == MessageRole::User {
                        "USER_EXPLICIT"
                    } else {
                        "MODEL"
                    };
                    let step_type = if msg.role == MessageRole::User {
                        "USER_INPUT"
                    } else {
                        "PLANNER_RESPONSE"
                    };

                    let content_payload = if msg.role == MessageRole::User {
                        format!("<USER_REQUEST>\n{}\n</USER_REQUEST>", msg.content)
                    } else {
                        msg.content.clone()
                    };

                    let line_obj = serde_json::json!({
                        "step_index": idx,
                        "source": source,
                        "type": step_type,
                        "status": "DONE",
                        "created_at": msg.created_at,
                        "content": content_payload
                    });

                    if let Ok(line_str) = serde_json::to_string(&line_obj) {
                        jsonl_lines.push(line_str);
                    }
                }

                if !jsonl_lines.is_empty() {
                    let full_content = jsonl_lines.join("\n") + "\n";
                    let _ = fs::write(&transcript_file, &full_content);
                    let _ = fs::write(&transcript_full_file, &full_content);
                }
            }

            // 生成有效的 Protobuf raw_summary（tag 1: 标题字符串）
            let title_bytes = title.as_bytes();
            let mut raw_summary_bytes = Vec::new();
            raw_summary_bytes.push(0x0a); // field 1 (string, wire type 2)
            let mut len = title_bytes.len();
            while len >= 0x80 {
                raw_summary_bytes.push(((len & 0x7f) | 0x80) as u8);
                len >>= 7;
            }
            raw_summary_bytes.push(len as u8);
            raw_summary_bytes.extend_from_slice(title_bytes);

            // 6. 写入目标 SQLite 数据库
            let insert_res = target_tx.execute(
                "INSERT INTO conversation_summaries (
                    conversation_id, title, preview, step_count, last_modified_time,
                    workspace_uris, status, source, project_id, agent_name,
                    parent_conversation_id, nesting_depth, battle_id, winning_conversation_id,
                    not_fully_idle, killed, last_user_input_time, last_user_input_step_index,
                    app_data_dir, raw_summary, group_id
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18,
                    ?19, ?20, ?21
                ) ON CONFLICT(conversation_id) DO UPDATE SET
                    title = excluded.title,
                    preview = excluded.preview,
                    step_count = MAX(conversation_summaries.step_count, excluded.step_count),
                    last_modified_time = excluded.last_modified_time,
                    workspace_uris = excluded.workspace_uris,
                    status = CASE WHEN conversation_summaries.status = 'CASCADE_RUN_STATUS_RUNNING' THEN conversation_summaries.status ELSE excluded.status END,
                    raw_summary = COALESCE(conversation_summaries.raw_summary, excluded.raw_summary)",
                params![
                    &ag_id,
                    &title,
                    &preview,
                    step_count,
                    &now_str,
                    &workspace_uris,
                    "CASCADE_RUN_STATUS_IDLE",
                    "",
                    "outside-of-project",
                    "",
                    "",
                    0i64,
                    "",
                    "",
                    0i64,
                    0i64,
                    &now_str,
                    0i64,
                    "antigravity",
                    Some(raw_summary_bytes),
                    ""
                ],
            );

            match insert_res {
                Ok(_) => {
                    published_count += 1;
                    if exists_in_target {
                        updated_count += 1;
                    } else {
                        new_count += 1;
                    }
                }
                Err(e) => {
                    warnings.push(format!("写入会话 “{}” 失败: {}", title, e));
                }
            }
        }

        target_tx
            .commit()
            .map_err(|e| format!("写入 Antigravity 数据库失败: {}", e))?;

        if Self::is_antigravity_running() {
            warnings.push("提示：Antigravity 客户端正在运行。会话数据已写入本地数据库与日志！若客户端侧边栏未自动刷新，按快捷键 Ctrl+R 重载或重启客户端即可查看。".to_string());
        }

        Ok(AntigravityPublishResult {
            published_count,
            new_count,
            updated_count,
            backup_path: backup_path_str,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;

    #[test]
    #[ignore = "Manual integration test requiring a local Antigravity installation"]
    fn test_preview_real() {
        let paths = AppPaths::discover(None).unwrap();
        let db = Database::new(&paths.database, &paths.backups).unwrap();
        let home = paths.antigravity_home();
        let publisher = AntigravityPublisher::new(&db, Some(&home), &paths.backups);
        let preview = publisher.preview(&[]);
        println!("Preview result: {:?}", preview);
        assert!(preview.is_ok());
    }

    #[test]
    fn test_publish_flow() {
        let temp_dir = std::env::temp_dir().join(format!("ag_test_{}", uuid::Uuid::new_v4()));
        let temp_ag_home = temp_dir.join("antigravity");
        let temp_backup = temp_dir.join("backups");
        let temp_db_path = temp_dir.join("test.db");
        fs::create_dir_all(&temp_ag_home).unwrap();
        fs::create_dir_all(&temp_backup).unwrap();

        let db = Database::new(&temp_db_path, &temp_backup).unwrap();
        let repo = Repository::new(&db);
        repo.sync_import_conversation(
            "AG Test Conversation",
            "antigravity_transcript",
            "test_ag_1",
            &[crate::persistence::ImportedMessagePayload {
                role: "user".to_string(),
                content: "Hello".to_string(),
                external_id: "ext1".to_string(),
                parent_external_id: None,
                created_at: None,
            }],
            Some("ext1"),
            None,
            "antigravity",
        )
        .unwrap();

        let publisher = AntigravityPublisher::new(&db, Some(&temp_ag_home), &temp_backup);
        let preview = publisher.preview(&[]).unwrap();
        assert!(!preview.candidates.is_empty());

        let candidate_ids: Vec<String> = preview
            .candidates
            .iter()
            .map(|c| c.conversation_id.clone())
            .collect();
        let res = publisher.publish(&candidate_ids);
        assert!(res.is_ok());

        let res = res.unwrap();
        assert_eq!(res.published_count, candidate_ids.len());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
