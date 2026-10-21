use chrono::{Local, SecondsFormat};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::domain::MessageRole;
use crate::persistence::{Database, Repository};
use crate::services::ClaudeClientController;

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("写入 {} 失败: {}", path.display(), e))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePublishCandidate {
    pub conversation_id: String,
    pub session_id: String,
    pub title: String,
    pub project_path: Option<String>,
    pub message_count: usize,
    pub exists_in_target: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePublishPreview {
    pub candidates: Vec<ClaudePublishCandidate>,
    pub total_count: usize,
    pub new_count: usize,
    pub existing_count: usize,
    pub target_claude_home: String,
    pub is_claude_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePublishResult {
    pub published_count: usize,
    pub new_count: usize,
    pub updated_count: usize,
    pub backup_path: Option<String>,
    pub warnings: Vec<String>,
}

pub struct ClaudePublisher<'a> {
    db: &'a Database,
    claude_home: PathBuf,
    backup_root: PathBuf,
}

impl<'a> ClaudePublisher<'a> {
    pub fn new(db: &'a Database, custom_home: Option<&Path>, backup_root: &Path) -> Self {
        let home = if let Some(p) = custom_home {
            p.to_path_buf()
        } else {
            let portable = PathBuf::from("./data/claude");
            if portable.is_dir() {
                portable
            } else if let Ok(env_home) = std::env::var("CLAUDE_HOME") {
                PathBuf::from(env_home)
            } else if let Ok(env_home) = std::env::var("CLAUDE_CONFIG_DIR") {
                PathBuf::from(env_home)
            } else {
                dirs::home_dir()
                    .map(|h| h.join(".claude"))
                    .unwrap_or_else(|| PathBuf::from(".claude"))
            }
        };

        Self {
            db,
            claude_home: home,
            backup_root: backup_root.to_path_buf(),
        }
    }

    /// 从消息记录或会话映射获取 Claude session_id
    fn resolve_claude_id(conn: &Connection, conversation_id: &str) -> String {
        let stmt = conn
            .prepare(
                "SELECT source_external_id FROM messages
                 WHERE conversation_id = ?1
                   AND source_type = 'claude_code'
                   AND source_external_id IS NOT NULL
                 LIMIT 1",
            )
            .ok();

        if let Some(mut s) = stmt {
            let ext_res = s.query_row(params![conversation_id], |row| row.get::<_, String>(0));
            if let Ok(ext_id) = ext_res {
                // 格式：claude_code:<session_id>:<uuid>
                let parts: Vec<&str> = ext_id.split(':').collect();
                if parts.len() >= 2 && !parts[1].trim().is_empty() {
                    return parts[1].trim().to_string();
                }
            }
        }

        conversation_id.to_string()
    }

    /// 扫描当前 Claude 配置目录下已存在的所有 session_id 与其所在的项目目录
    fn scan_existing_sessions(&self) -> HashMap<String, PathBuf> {
        let mut map = HashMap::new();
        let projects_dir = self.claude_home.join("projects");
        if !projects_dir.is_dir() {
            return map;
        }

        if let Ok(proj_entries) = fs::read_dir(&projects_dir) {
            for pe in proj_entries.flatten() {
                let proj_path = pe.path();
                if !proj_path.is_dir() {
                    continue;
                }

                if let Ok(sess_entries) = fs::read_dir(&proj_path) {
                    for se in sess_entries.flatten() {
                        let sp = se.path();
                        if sp.is_file()
                            && sp
                                .extension()
                                .and_then(|s| s.to_str())
                                .map(|s| s.eq_ignore_ascii_case("jsonl"))
                                .unwrap_or(false)
                        {
                            if let Some(stem) = sp.file_stem().and_then(|s| s.to_str()) {
                                if !stem.is_empty() {
                                    map.insert(stem.to_string(), proj_path.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        map
    }

    /// 根据 project_path 查找已存在的 Claude 项目目录，若无则生成规范的 sanitized 目录名
    fn resolve_project_dir(&self, project_path: Option<&str>) -> PathBuf {
        let projects_dir = self.claude_home.join("projects");
        let _ = fs::create_dir_all(&projects_dir);

        let target_path = project_path.unwrap_or("").trim();
        if target_path.is_empty() {
            return projects_dir.join("default");
        }

        let canonical = projects_dir.join(Self::project_dir_name(target_path));
        if canonical.is_dir() {
            return canonical;
        }

        // A folder created by another tool may use a different name; match it by cwd.
        if let Ok(entries) = fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let Ok(sess_entries) = fs::read_dir(&p) else {
                    continue;
                };
                let first_transcript = sess_entries.flatten().map(|se| se.path()).find(|sp| {
                    sp.is_file()
                        && sp
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.eq_ignore_ascii_case("jsonl"))
                            .unwrap_or(false)
                });
                if let Some(cwd) = first_transcript.and_then(|sp| Self::transcript_cwd(&sp)) {
                    if cwd.trim().eq_ignore_ascii_case(target_path) {
                        return p;
                    }
                }
            }
        }

        canonical
    }

    /// Claude Code's project folder name: every character other than an ASCII letter
    /// or digit becomes `-`, e.g. `G:\web\demo` -> `G--web-demo`.
    pub fn project_dir_name(cwd: &str) -> String {
        let name: String = cwd
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        if name.is_empty() {
            "default".to_string()
        } else {
            name
        }
    }

    fn transcript_cwd(path: &Path) -> Option<String> {
        let reader = BufReader::new(File::open(path).ok()?);
        reader.lines().take(200).flatten().find_map(|line| {
            serde_json::from_str::<serde_json::Value>(&line)
                .ok()?
                .get("cwd")?
                .as_str()
                .map(str::to_string)
                .filter(|c| !c.trim().is_empty())
        })
    }

    /// 预览将要同步/发布至 Claude 的会话列表
    pub fn preview(&self, selected_ids: &[String]) -> Result<ClaudePublishPreview, String> {
        let is_running = ClaudeClientController::new().is_running();
        let conn = self.db.lock_conn();
        let existing_sessions = self.scan_existing_sessions();

        let query = if selected_ids.is_empty() {
            "SELECT id, title, project_path, updated_at FROM conversations WHERE source_kind = 'claude' ORDER BY updated_at DESC".to_string()
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
            let sess_id = Self::resolve_claude_id(&conn, &conv_id);
            let exists = existing_sessions.contains_key(&sess_id);

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

            candidates.push(ClaudePublishCandidate {
                conversation_id: conv_id,
                session_id: sess_id,
                title,
                project_path,
                message_count: msg_count,
                exists_in_target: exists,
                updated_at,
            });
        }

        let total_count = candidates.len();
        let target_claude_home = self.claude_home.to_string_lossy().to_string();

        Ok(ClaudePublishPreview {
            candidates,
            total_count,
            new_count,
            existing_count,
            target_claude_home,
            is_claude_running: is_running,
        })
    }

    /// 执行同步/发布会话至 Claude
    pub fn publish(
        &self,
        selected_ids: &[String],
        auto_restart_client: bool,
    ) -> Result<ClaudePublishResult, String> {
        let controller = ClaudeClientController::new();
        let was_running = auto_restart_client && controller.is_running();
        if was_running {
            controller.stop(10.0)?;
        }
        let mut result = self.publish_sessions(selected_ids);
        if was_running {
            if let Err(error) = controller.start(10.0) {
                match &mut result {
                    Ok(report) => report.warnings.push(format!("Claude 重启失败: {}", error)),
                    Err(message) => message.push_str(&format!("; Claude 重启失败: {}", error)),
                }
            }
        }
        result
    }

    fn publish_sessions(&self, selected_ids: &[String]) -> Result<ClaudePublishResult, String> {
        let repo = Repository::new(self.db);

        if selected_ids.is_empty() {
            return Err("未选择任何要发布至 Claude 的会话".to_string());
        }

        // 1. 确保目标目录结构就绪
        fs::create_dir_all(&self.claude_home).map_err(|e| {
            format!(
                "无法创建 Claude 根目录 {}: {}",
                self.claude_home.display(),
                e
            )
        })?;
        let projects_dir = self.claude_home.join("projects");
        fs::create_dir_all(&projects_dir).map_err(|e| {
            format!(
                "无法创建 Claude projects 目录 {}: {}",
                projects_dir.display(),
                e
            )
        })?;

        // Back up history, titles, and original transcripts before changing any files.
        let history_file = self.claude_home.join("history.jsonl");

        let existing_sessions = self.scan_existing_sessions();

        // 3. 读取所选会话元数据
        let candidate_meta: Vec<(String, String, String, Option<String>)> = {
            let conn = self.db.lock_conn();
            let mut list = Vec::new();
            for conv_id in selected_ids {
                let sess_id = Self::resolve_claude_id(&conn, conv_id);
                let conv_res = conn.query_row(
                    "SELECT title, project_path FROM conversations WHERE id = ?1",
                    params![conv_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
                );
                match conv_res {
                    Ok((title, project_path)) => {
                        list.push((conv_id.clone(), sess_id, title, project_path));
                    }
                    Err(_) => {
                        // 会话不存在
                    }
                }
            }
            list
        };

        let mut published_count = 0;
        let mut new_count = 0;
        let mut updated_count = 0;
        let mut warnings = Vec::new();

        let backup_dir = self.backup_root.join("claude").join(format!(
            "publish_{}_{}",
            Local::now().format("%Y%m%d_%H%M%S"),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&backup_dir).map_err(|e| format!("创建 Claude 备份失败: {}", e))?;
        let mut affected_files = vec![history_file.clone()];
        for (_, session_id, _, project_path) in &candidate_meta {
            if session_id.contains(['/', '\\', ':']) || session_id == "." || session_id == ".." {
                return Err("无效的 Claude 会话 ID".to_string());
            }
            let project = existing_sessions
                .get(session_id)
                .cloned()
                .unwrap_or_else(|| self.resolve_project_dir(project_path.as_deref()));
            affected_files.push(project.join(format!("{}.jsonl", session_id)));
            affected_files.push(project.join(session_id).join("custom-title.json"));
        }
        for source in &affected_files {
            if source.exists() {
                let relative = source
                    .strip_prefix(&self.claude_home)
                    .map_err(|e| e.to_string())?;
                let destination = backup_dir.join(relative);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                fs::copy(source, &destination)
                    .map_err(|e| format!("备份 {} 失败: {}", source.display(), e))?;
            }
        }
        let backup_path_str = Some(backup_dir.to_string_lossy().to_string());

        // 4. 读取现有 history.jsonl 便于增量或更新
        let mut history_entries: Vec<String> = Vec::new();
        let mut history_index_by_session: HashMap<String, usize> = HashMap::new();
        if history_file.is_file() {
            {
                let file = File::open(&history_file).map_err(|e| e.to_string())?;
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    let line_res = line.map_err(|e| e.to_string())?;
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line_res) {
                        if let Some(sid) = val.get("sessionId").and_then(|s| s.as_str()) {
                            history_index_by_session.insert(sid.to_string(), history_entries.len());
                        }
                    }
                    history_entries.push(line_res);
                }
            }
        }

        let now_millis = chrono::Utc::now().timestamp_millis();

        // 5. 遍历处理每个会话
        for (conv_id, sess_id, title, project_path) in candidate_meta {
            let exists_in_target = existing_sessions.contains_key(&sess_id);
            let proj_dir = if let Some(existing_proj) = existing_sessions.get(&sess_id) {
                existing_proj.clone()
            } else {
                self.resolve_project_dir(project_path.as_deref())
            };

            fs::create_dir_all(&proj_dir).map_err(|e| e.to_string())?;

            // 获取该会话的消息流
            let messages = repo.load_active_path(&conv_id).map_err(|e| e.to_string())?;
            if messages.is_empty() && !exists_in_target {
                warnings.push(format!("会话 “{}” 中没有可用消息，跳过", title));
                continue;
            }

            // 写出 <sess_id>.jsonl
            let sess_jsonl_path = proj_dir.join(format!("{}.jsonl", sess_id));
            let cwd_str = project_path.as_deref().unwrap_or("").trim();

            let mut lines = Vec::new();
            let mut parent_uuid: Option<String> = None;
            for (idx, msg) in messages.iter().enumerate() {
                let msg_created_at = msg.created_at.to_rfc3339_opts(SecondsFormat::Millis, true);

                if msg.role == MessageRole::User {
                    let user_obj = serde_json::json!({
                        "type": "user",
                        "isSidechain": false,
                        "userType": "external",
                        "message": {
                            "role": "user",
                            "content": msg.content
                        },
                        "uuid": msg.id,
                        "sessionId": sess_id,
                        "parentUuid": parent_uuid,
                        "timestamp": msg_created_at,
                        "cwd": cwd_str
                    });
                    if let Ok(s) = serde_json::to_string(&user_obj) {
                        lines.push(s);
                    }
                } else if msg.role == MessageRole::Assistant {
                    let asst_obj = serde_json::json!({
                        "type": "assistant",
                        "isSidechain": false,
                        "userType": "external",
                        "requestId": format!("req_{}_{}", sess_id, idx),
                        "uuid": msg.id,
                        "sessionId": sess_id,
                        "parentUuid": parent_uuid,
                        "cwd": cwd_str,
                        "timestamp": msg_created_at,
                        "message": {
                            "id": format!("msg_{}", msg.id.replace('-', "")),
                            "type": "message",
                            "role": "assistant",
                            "model": msg.model.clone().unwrap_or_else(|| "<synthetic>".to_string()),
                            "content": [
                                {
                                    "type": "text",
                                    "text": msg.content
                                }
                            ]
                        }
                    });
                    if let Ok(s) = serde_json::to_string(&asst_obj) {
                        lines.push(s);
                    }
                }
                if matches!(msg.role, MessageRole::User | MessageRole::Assistant) {
                    parent_uuid = Some(msg.id.clone());
                }
            }

            // Claude Code shows the newest `custom-title` line in its session list.
            if let Ok(s) = serde_json::to_string(&serde_json::json!({
                "type": "custom-title",
                "customTitle": title,
                "sessionId": sess_id,
            })) {
                lines.push(s);
            }

            // Existing native transcripts may contain newer messages and rich tool metadata.
            // Preserve them byte-for-byte; only update their title and history entry.
            if !exists_in_target && !lines.is_empty() {
                let content_str = lines.join("\n") + "\n";
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&sess_jsonl_path)
                    .map_err(|e| {
                        format!(
                            "创建 {} 失败（现有会话不会被覆盖）: {}",
                            sess_jsonl_path.display(),
                            e
                        )
                    })?;
                let written = file
                    .write_all(content_str.as_bytes())
                    .and_then(|_| file.sync_all());
                drop(file);
                if let Err(error) = written {
                    let _ = fs::remove_file(&sess_jsonl_path);
                    return Err(format!("写入新会话失败: {}", error));
                }
            }

            // 写出/更新 <session_id>/custom-title.json
            let custom_title_dir = proj_dir.join(&sess_id);
            fs::create_dir_all(&custom_title_dir).map_err(|e| e.to_string())?;
            let custom_title_path = custom_title_dir.join("custom-title.json");
            let mut title_json = if custom_title_path.is_file() {
                serde_json::from_slice::<serde_json::Value>(
                    &fs::read(&custom_title_path).map_err(|e| e.to_string())?,
                )
                .map_err(|e| format!("标题文件无效，未覆盖: {}", e))?
            } else {
                serde_json::json!({})
            };
            if !title_json.is_object() {
                return Err("标题文件不是 JSON 对象，未覆盖".to_string());
            }
            title_json["customTitle"] = serde_json::json!(title);
            write_atomic(
                &custom_title_path,
                &serde_json::to_vec_pretty(&title_json).map_err(|e| e.to_string())?,
            )?;

            // 更新 history_entries
            let history_item = serde_json::json!({
                "display": title,
                "project": cwd_str,
                "sessionId": sess_id,
                "timestamp": now_millis
            });

            if let Some(&idx) = history_index_by_session.get(&sess_id) {
                let mut existing: serde_json::Value =
                    serde_json::from_str(&history_entries[idx]).map_err(|e| e.to_string())?;
                existing["display"] = serde_json::json!(title);
                history_entries[idx] =
                    serde_json::to_string(&existing).map_err(|e| e.to_string())?;
            } else {
                history_index_by_session.insert(sess_id.clone(), history_entries.len());
                history_entries
                    .push(serde_json::to_string(&history_item).map_err(|e| e.to_string())?);
            }

            published_count += 1;
            if exists_in_target {
                updated_count += 1;
            } else {
                new_count += 1;
            }
        }

        // 6. 写回 history.jsonl
        if !history_entries.is_empty() {
            let full_history = history_entries.join("\n") + "\n";
            write_atomic(&history_file, full_history.as_bytes())?;
        }

        Ok(ClaudePublishResult {
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

    #[test]
    fn test_claude_publish_flow() {
        let temp_dir =
            std::env::temp_dir().join(format!("cv_test_claude_pub_{}", uuid::Uuid::new_v4()));
        let temp_claude_home = temp_dir.join("claude");
        let temp_backup = temp_dir.join("backups");
        let temp_db_path = temp_dir.join("test.db");
        fs::create_dir_all(&temp_claude_home).unwrap();
        fs::create_dir_all(&temp_backup).unwrap();

        let db = Database::new(&temp_db_path, &temp_backup).unwrap();
        let repo = Repository::new(&db);
        repo.sync_import_conversation(
            "Claude Casting Mold Optimization",
            "claude_code",
            "test_claude_sess_1",
            &[
                crate::persistence::ImportedMessagePayload {
                    role: "user".to_string(),
                    content: "How to fix casting mold?".to_string(),
                    external_id: "ext1".to_string(),
                    parent_external_id: None,
                    created_at: None,
                },
                crate::persistence::ImportedMessagePayload {
                    role: "assistant".to_string(),
                    content: "Adjust flow rate and cooling timing.".to_string(),
                    external_id: "ext2".to_string(),
                    parent_external_id: Some("ext1".to_string()),
                    created_at: None,
                },
            ],
            Some("ext2"),
            Some("C:\\projects\\molds"),
            "claude",
        )
        .unwrap();

        let publisher = ClaudePublisher::new(&db, Some(&temp_claude_home), &temp_backup);
        let preview = publisher.preview(&[]).unwrap();
        assert_eq!(preview.total_count, 1);
        assert_eq!(
            preview.candidates[0].title,
            "Claude Casting Mold Optimization"
        );

        let res = publisher
            .publish(&[preview.candidates[0].conversation_id.clone()], false)
            .unwrap();
        assert_eq!(res.published_count, 1);
        assert_eq!(res.new_count, 1);

        // Verify history.jsonl
        let history_file = temp_claude_home.join("history.jsonl");
        assert!(history_file.is_file());
        let content = fs::read_to_string(&history_file).unwrap();
        assert!(content.contains("Claude Casting Mold Optimization"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
