use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::persistence::{Database, ImportedMessagePayload, Repository};
use crate::util::{normalize_timestamp, percent_decode, text_from_value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityProject {
    pub project_id: String,
    pub name: String,
    pub root_paths: Vec<String>,
}

impl AntigravityProject {
    pub fn primary_path(&self) -> &str {
        self.root_paths.first().map(|s| s.as_str()).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityCandidate {
    pub conversation_id: String,
    pub title: String,
    pub preview: String,
    pub step_count: i64,
    pub last_modified_time: String,
    pub project_path: Option<String>,
    pub status: String,
    pub exists_locally: bool,
    pub transcript_path: String,
    pub has_full_transcript: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityRecoveryReport {
    pub candidates: Vec<AntigravityCandidate>,
    pub total_scanned: usize,
    pub home_directory: String,
    pub excluded_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityRecoveryResult {
    pub conversation_count: usize,
    pub new_conversation_count: usize,
    pub message_count: usize,
    pub title_update_count: usize,
    pub skipped_count: usize,
    pub warnings: Vec<String>,
}

pub struct AntigravityRecoveryService<'a> {
    db: &'a Database,
    antigravity_home: PathBuf,
}

impl<'a> AntigravityRecoveryService<'a> {
    pub const SOURCE_TYPE: &'static str = "antigravity_transcript";
    pub const SOURCE_KIND: &'static str = "antigravity";

    pub fn new(db: &'a Database, custom_home: Option<&Path>) -> Self {
        let home = if let Some(p) = custom_home {
            p.to_path_buf()
        } else {
            // Priority: Portable ./data/antigravity -> %USERPROFILE%/.gemini/antigravity
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
        }
    }

    pub fn home_directory(&self) -> String {
        self.antigravity_home.to_string_lossy().to_string()
    }

    pub fn current_projects(&self) -> Vec<AntigravityProject> {
        let mut map: HashMap<String, String> = HashMap::new();

        let summaries_db_path = self.antigravity_home.join("conversation_summaries.db");
        if summaries_db_path.is_file() {
            if let Ok(conn) = Connection::open_with_flags(
                &summaries_db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                if let Ok(mut stmt) =
                    conn.prepare("SELECT DISTINCT workspace_uris FROM conversation_summaries")
                {
                    if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                        for uri_raw in rows.flatten() {
                            if let Some(path) = Self::decode_workspace_uri(&uri_raw) {
                                let name = Path::new(&path)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(&path)
                                    .to_string();
                                map.insert(path, name);
                            }
                        }
                    }
                }
            }
        }

        let mut projects: Vec<AntigravityProject> = map
            .into_iter()
            .map(|(path, name)| AntigravityProject {
                project_id: path.clone(),
                name,
                root_paths: vec![path],
            })
            .collect();

        projects.sort_by(|a, b| a.name.cmp(&b.name));
        projects
    }

    /// 扫描 Antigravity 历史会话（包含数据库索引与 brain 目录全量兜底）
    pub fn scan(&self) -> AntigravityRecoveryReport {
        let repo = Repository::new(self.db);
        let mut warnings = Vec::new();
        let mut candidates = Vec::new();
        let mut seen_cids: HashSet<String> = HashSet::new();
        let mut excluded_count = 0;

        let excluded_ids = repo
            .excluded_source_ids(Self::SOURCE_TYPE)
            .unwrap_or_default();
        let imported_ids = repo
            .imported_source_ids(Self::SOURCE_TYPE)
            .unwrap_or_default();

        let summaries_db_path = self.antigravity_home.join("conversation_summaries.db");
        if summaries_db_path.is_file() {
            // 以只读模式连接 conversation_summaries.db
            match Connection::open_with_flags(
                &summaries_db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                Ok(conn) => {
                    let stmt = match conn.prepare(
                        "SELECT conversation_id, title, preview, step_count, last_modified_time,
                                workspace_uris, status, nesting_depth, parent_conversation_id
                         FROM conversation_summaries
                         ORDER BY last_modified_time DESC",
                    ) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            warnings.push(format!("查询 Antigravity 会话汇总失败：{}", e));
                            None
                        }
                    };

                    if let Some(mut s) = stmt {
                        // Columns may be NULL or use another storage class in other
                        // Antigravity versions; read them leniently instead of dropping rows.
                        let rows = s.query_map([], |row| {
                            let text = |idx: usize| -> rusqlite::Result<String> {
                                Ok(text_from_value(&row.get::<_, Value>(idx)?).unwrap_or_default())
                            };
                            let int = |idx: usize| -> rusqlite::Result<i64> {
                                Ok(match row.get::<_, Value>(idx)? {
                                    Value::Integer(i) => i,
                                    Value::Real(f) => f as i64,
                                    Value::Text(t) => t.trim().parse().unwrap_or(0),
                                    _ => 0,
                                })
                            };
                            Ok((
                                text(0)?,
                                text(1)?,
                                text(2)?,
                                int(3)?,
                                normalize_timestamp(&text(4)?),
                                text(5)?,
                                text(6)?,
                                int(7)?,
                                text(8)?,
                            ))
                        });

                        if let Ok(mapped) = rows {
                            for item in mapped.flatten() {
                                let (
                                    cid,
                                    title,
                                    preview,
                                    step_count,
                                    last_modified,
                                    workspace_uris_raw,
                                    status,
                                    nesting_depth,
                                    _parent_id,
                                ) = item;

                                if nesting_depth > 0 || cid.is_empty() {
                                    continue;
                                }

                                if excluded_ids.contains(&cid) {
                                    excluded_count += 1;
                                    seen_cids.insert(cid);
                                    continue;
                                }

                                let logs_dir = self
                                    .antigravity_home
                                    .join("brain")
                                    .join(&cid)
                                    .join(".system_generated")
                                    .join("logs");
                                let full_path = logs_dir.join("transcript_full.jsonl");
                                let regular_path = logs_dir.join("transcript.jsonl");

                                let (chosen_transcript, has_full) = if full_path.is_file() {
                                    (full_path, true)
                                } else if regular_path.is_file() {
                                    (regular_path, false)
                                } else {
                                    continue;
                                };

                                let project_path = Self::decode_workspace_uri(&workspace_uris_raw);
                                let final_title = if title.trim().is_empty() {
                                    if !preview.trim().is_empty() {
                                        let p = preview.trim();
                                        let char_count = p.chars().count();
                                        if char_count > 60 {
                                            let truncated: String = p.chars().take(60).collect();
                                            format!("{}...", truncated)
                                        } else {
                                            p.to_string()
                                        }
                                    } else {
                                        "Antigravity 会话".to_string()
                                    }
                                } else {
                                    title.trim().to_string()
                                };

                                seen_cids.insert(cid.clone());
                                candidates.push(AntigravityCandidate {
                                    conversation_id: cid.clone(),
                                    title: final_title,
                                    preview,
                                    step_count,
                                    last_modified_time: last_modified,
                                    project_path,
                                    status,
                                    exists_locally: imported_ids.contains(&cid),
                                    transcript_path: chosen_transcript
                                        .to_string_lossy()
                                        .to_string(),
                                    has_full_transcript: has_full,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    warnings.push(format!("打开 Antigravity 索引数据库失败：{}", e));
                }
            }
        } else {
            warnings.push(format!(
                "未在 {} 找到 Antigravity 会话索引数据库，使用 brain 目录兜底扫描",
                self.antigravity_home.display()
            ));
        }

        // 类似 Codex 的 rollout 目录兜底机制：扫描 brain/ 下尚未被索引的会话
        let brain_dir = self.antigravity_home.join("brain");
        if brain_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&brain_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let cid = match path.file_name().and_then(|n| n.to_str()) {
                        Some(name) => name.to_string(),
                        None => continue,
                    };
                    if cid.starts_with('.') || seen_cids.contains(&cid) {
                        continue;
                    }
                    if excluded_ids.contains(&cid) {
                        excluded_count += 1;
                        seen_cids.insert(cid);
                        continue;
                    }

                    let logs_dir = path.join(".system_generated").join("logs");
                    let full_path = logs_dir.join("transcript_full.jsonl");
                    let regular_path = logs_dir.join("transcript.jsonl");

                    let (chosen_transcript, has_full) = if full_path.is_file() {
                        (full_path, true)
                    } else if regular_path.is_file() {
                        (regular_path, false)
                    } else {
                        continue;
                    };

                    let (title, preview, mod_time) =
                        Self::quick_peek_transcript(&chosen_transcript);
                    seen_cids.insert(cid.clone());
                    candidates.push(AntigravityCandidate {
                        conversation_id: cid.clone(),
                        title,
                        preview,
                        step_count: 0,
                        last_modified_time: mod_time,
                        project_path: None,
                        status: "DONE".to_string(),
                        exists_locally: imported_ids.contains(&cid),
                        transcript_path: chosen_transcript.to_string_lossy().to_string(),
                        has_full_transcript: has_full,
                    });
                }
            }
        }

        // 按最后更新时间倒序排列
        candidates.sort_by(|a, b| b.last_modified_time.cmp(&a.last_modified_time));
        let total_scanned = candidates.len();

        AntigravityRecoveryReport {
            candidates,
            total_scanned,
            home_directory: self.home_directory(),
            excluded_count,
            warnings,
        }
    }

    /// 自动向本地数据库同步已存在会话的外部最新标题
    pub fn sync_existing_titles(&self, candidates: &[AntigravityCandidate]) -> usize {
        let repo = Repository::new(self.db);
        let mut titles = HashMap::new();
        for c in candidates {
            if c.exists_locally {
                titles.insert(c.conversation_id.clone(), c.title.clone());
            }
        }
        repo.sync_import_titles(Self::SOURCE_TYPE, &titles)
            .unwrap_or(0)
    }

    /// 快速探测日志文件提取首条用户消息作为标题与修改时间
    fn quick_peek_transcript(path: &Path) -> (String, String, String) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => {
                return (
                    "Antigravity 会话".to_string(),
                    String::new(),
                    chrono::Utc::now().to_rfc3339(),
                )
            }
        };
        let mod_time = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
            .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339());

        let reader = BufReader::new(file);
        for line_res in reader.lines() {
            if let Ok(line) = line_res {
                if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
                    if item.get("type").and_then(|t| t.as_str()) == Some("USER_INPUT") {
                        let raw = item.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        let cleaned = Self::clean_user_input(raw);
                        if !cleaned.is_empty() {
                            let title = if cleaned.chars().count() > 40 {
                                format!("{}...", cleaned.chars().take(40).collect::<String>())
                            } else {
                                cleaned.clone()
                            };
                            let preview = if cleaned.chars().count() > 100 {
                                format!("{}...", cleaned.chars().take(100).collect::<String>())
                            } else {
                                cleaned
                            };
                            return (title, preview, mod_time);
                        }
                    }
                }
            }
        }
        ("Antigravity 会话".to_string(), String::new(), mod_time)
    }

    /// 执行选定候选会话的恢复与导入
    pub fn recover(&self, candidates: &[AntigravityCandidate]) -> AntigravityRecoveryResult {
        let repo = Repository::new(self.db);
        let excluded_ids = repo
            .excluded_source_ids(Self::SOURCE_TYPE)
            .unwrap_or_default();

        let mut conversations = 0;
        let mut new_conversations = 0;
        let mut messages = 0;
        let mut title_updates = 0;
        let mut skipped = 0;
        let mut warnings = Vec::new();

        for candidate in candidates {
            if excluded_ids.contains(&candidate.conversation_id) {
                skipped += 1;
                warnings.push(format!("“{}”已被用户删除，恢复已跳过", candidate.title));
                continue;
            }

            let path = Path::new(&candidate.transcript_path);
            let imported_messages = Self::parse_transcript(path);
            if imported_messages.is_empty() {
                skipped += 1;
                warnings.push(format!(
                    "“{}”没有可解析的用户提问或助手回复文本",
                    candidate.title
                ));
                continue;
            }

            let active_ext_id = imported_messages.last().map(|m| m.external_id.as_str());

            match repo.sync_import_conversation(
                &candidate.title,
                Self::SOURCE_TYPE,
                &candidate.conversation_id,
                &imported_messages,
                active_ext_id,
                candidate.project_path.as_deref(),
                Self::SOURCE_KIND,
            ) {
                Ok((_id, inserted, created, title_updated)) => {
                    conversations += 1;
                    if created {
                        new_conversations += 1;
                    }
                    messages += inserted;
                    if title_updated {
                        title_updates += 1;
                    }
                    if !created && inserted == 0 && !title_updated {
                        skipped += 1;
                    }
                }
                Err(e) => {
                    skipped += 1;
                    warnings.push(format!("“{}”恢复失败：{}", candidate.title, e));
                }
            }
        }

        AntigravityRecoveryResult {
            conversation_count: conversations,
            new_conversation_count: new_conversations,
            message_count: messages,
            title_update_count: title_updates,
            skipped_count: skipped,
            warnings,
        }
    }

    /// 解析 Antigravity transcript.jsonl / transcript_full.jsonl 文件
    pub fn parse_transcript(path: &Path) -> Vec<ImportedMessagePayload> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let mut messages: Vec<ImportedMessagePayload> = Vec::new();
        let mut prev_external_id: Option<String> = None;
        let reader = BufReader::new(file);

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(_) => continue,
            };

            let item: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let step_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let step_idx = item
                .get("step_index")
                .and_then(|s| s.as_i64())
                .unwrap_or(line_idx as i64);
            let created_at = item
                .get("created_at")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            if step_type == "USER_INPUT" {
                let raw_content = item.get("content").and_then(|c| c.as_str()).unwrap_or("");
                let cleaned = Self::clean_user_input(raw_content);
                if cleaned.is_empty() {
                    continue;
                }

                let ext_id = format!("user:{}", step_idx);
                messages.push(ImportedMessagePayload {
                    role: "user".to_string(),
                    content: cleaned,
                    external_id: ext_id.clone(),
                    parent_external_id: prev_external_id.clone(),
                    created_at,
                });
                prev_external_id = Some(ext_id);
            } else if step_type == "PLANNER_RESPONSE" {
                // 模型回答：若包含 content 文本则提取
                let raw_content = item
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .trim();
                if raw_content.is_empty() {
                    continue;
                }

                let ext_id = format!("assistant:{}", step_idx);
                messages.push(ImportedMessagePayload {
                    role: "assistant".to_string(),
                    content: raw_content.to_string(),
                    external_id: ext_id.clone(),
                    parent_external_id: prev_external_id.clone(),
                    created_at,
                });
                prev_external_id = Some(ext_id);
            }
        }

        messages
    }

    /// 清理用户输入，剔除 <USER_REQUEST> 外部的系统注入元数据
    pub fn clean_user_input(raw: &str) -> String {
        let text = raw.trim();
        if let Some(start_pos) = text.find("<USER_REQUEST>") {
            let after = &text[start_pos + "<USER_REQUEST>".len()..];
            if let Some(end_pos) = after.find("</USER_REQUEST>") {
                let extracted = after[..end_pos].trim();
                if !extracted.is_empty() {
                    return extracted.to_string();
                }
            }
        }

        // 回退逻辑：剔除可能存在的注入标签
        let mut cleaned = text.to_string();
        if let Some(idx) = cleaned.find("<ADDITIONAL_METADATA>") {
            cleaned.truncate(idx);
        }
        if let Some(idx) = cleaned.find("<USER_SETTINGS_CHANGE>") {
            cleaned.truncate(idx);
        }
        cleaned.trim().to_string()
    }

    /// 解码 Antigravity 的 workspace_uris
    /// 示例：`["file:///c%3A/Projects"]` -> `C:\Projects`
    pub fn decode_workspace_uri(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let uri_str = if trimmed.starts_with('[') {
            if let Ok(vec) = serde_json::from_str::<Vec<String>>(trimmed) {
                vec.into_iter().next().unwrap_or_default()
            } else {
                trimmed
                    .trim_matches(|c| c == '[' || c == ']' || c == '"' || c == '\'')
                    .to_string()
            }
        } else {
            trimmed.to_string()
        };

        if uri_str.is_empty() {
            return None;
        }

        let mut path_str = uri_str;
        if path_str.starts_with("file:///") {
            path_str = path_str["file:///".len()..].to_string();
        } else if path_str.starts_with("file://") {
            path_str = path_str["file://".len()..].to_string();
        }

        path_str = percent_decode(&path_str);

        #[cfg(windows)]
        {
            if path_str.len() >= 2 && path_str.chars().nth(1) == Some(':') {
                path_str = path_str.replace('/', "\\");
                let (drive, rest) = path_str.split_at(1);
                path_str = format!("{}{}", drive.to_uppercase(), rest);
            }
        }

        let p = PathBuf::from(&path_str);
        Some(p.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_user_input() {
        let raw = "<USER_REQUEST>\n请帮我优化代码\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\ntime\n</ADDITIONAL_METADATA>";
        assert_eq!(
            AntigravityRecoveryService::clean_user_input(raw),
            "请帮我优化代码"
        );

        let plain = "普通用户输入，没有包含任何标签";
        assert_eq!(
            AntigravityRecoveryService::clean_user_input(plain),
            "普通用户输入，没有包含任何标签"
        );
    }

    #[test]
    fn test_decode_workspace_uri() {
        let uri_json = r#"["file:///c%3A/Projects/val"]"#;
        let decoded = AntigravityRecoveryService::decode_workspace_uri(uri_json).unwrap();
        assert!(decoded.to_lowercase().contains("projects"));

        let single = "file:///c%3A/projects/demo";
        let dec2 = AntigravityRecoveryService::decode_workspace_uri(single).unwrap();
        assert!(dec2.to_lowercase().contains("demo"));
    }

    #[test]
    fn test_parse_and_recover_transcript() {
        use std::io::Write;
        let test_dir = std::env::temp_dir().join(format!("cv_test_ag_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&test_dir);

        let transcript_path = test_dir.join("transcript.jsonl");
        {
            let mut f = std::fs::File::create(&transcript_path).unwrap();
            writeln!(
                f,
                r#"{{"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>如何使用 Rust 恢复 Antigravity 对话？</USER_REQUEST>","created_at":"2026-09-22T08:00:00Z"}}"#
            ).unwrap();
            writeln!(
                f,
                r#"{{"step_index":1,"type":"PLANNER_RESPONSE","content":"通过读取 transcript.jsonl 并使用 SQLite 存储。","created_at":"2026-09-22T08:00:05Z"}}"#
            ).unwrap();
        }

        let parsed = AntigravityRecoveryService::parse_transcript(&transcript_path);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].role, "user");
        assert_eq!(parsed[0].content, "如何使用 Rust 恢复 Antigravity 对话？");
        assert_eq!(parsed[1].role, "assistant");
        assert_eq!(
            parsed[1].content,
            "通过读取 transcript.jsonl 并使用 SQLite 存储。"
        );

        // Database recovery test
        let db_path = test_dir.join("conversations.db");
        let backup_dir = test_dir.join("backups");
        let db = Database::new(&db_path, &backup_dir).unwrap();
        let service = AntigravityRecoveryService::new(&db, Some(&test_dir));

        let candidate = AntigravityCandidate {
            conversation_id: "test-ag-uuid-123".to_string(),
            title: "Rust 恢复 Antigravity 讨论".to_string(),
            preview: "如何使用 Rust 恢复 Antigravity 对话？".to_string(),
            step_count: 2,
            last_modified_time: "2026-09-22T08:00:05Z".to_string(),
            project_path: Some("C:\\Projects\\Context restoration app".to_string()),
            status: "CASCADE_RUN_STATUS_IDLE".to_string(),
            exists_locally: false,
            transcript_path: transcript_path.to_string_lossy().to_string(),
            has_full_transcript: false,
        };

        let result = service.recover(&[candidate]);
        assert_eq!(result.new_conversation_count, 1);
        assert_eq!(result.message_count, 2);

        // Verify FTS5 search finds the message
        let repo = Repository::new(&db);
        let search_res = repo.search_conversations("Antigravity", 10).unwrap();
        assert_eq!(search_res.len(), 1);
        assert_eq!(search_res[0].source_kind, "antigravity");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    #[ignore = "Manual integration test requiring local Antigravity history"]
    fn test_real_system_scan() {
        let test_dir =
            std::env::temp_dir().join(format!("cv_test_ag_real_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&test_dir);
        let db_path = test_dir.join("conversations.db");
        let backup_dir = test_dir.join("backups");
        let db = Database::new(&db_path, &backup_dir).unwrap();
        let service = AntigravityRecoveryService::new(&db, None);
        let report = service.scan();
        if Path::new(&report.home_directory).is_dir() {
            assert!(report.total_scanned > 0);
            println!(
                "Real system scan successfully discovered {} sessions from {}",
                report.total_scanned, report.home_directory
            );
        }
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
