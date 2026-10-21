use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::persistence::{Database, ImportedMessagePayload, Repository};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProject {
    pub project_id: String,
    pub name: String,
    pub root_paths: Vec<String>,
}

impl ClaudeProject {
    pub fn primary_path(&self) -> &str {
        self.root_paths.first().map(|s| s.as_str()).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCandidate {
    pub session_id: String,
    pub title: String,
    pub preview: String,
    pub message_count: usize,
    pub last_modified_time: String,
    pub project_path: Option<String>,
    pub exists_locally: bool,
    pub session_file_path: String,
    pub custom_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRecoveryReport {
    pub candidates: Vec<ClaudeCandidate>,
    pub total_scanned: usize,
    pub claude_home: String,
    pub excluded_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRecoveryResult {
    pub conversation_count: usize,
    pub new_conversation_count: usize,
    pub message_count: usize,
    pub title_update_count: usize,
    pub skipped_count: usize,
    pub warnings: Vec<String>,
}

fn safe_truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

#[derive(Default)]
struct SessionPeek {
    cwd: Option<String>,
    first_prompt: String,
    message_count: usize,
    custom_title: Option<String>,
}

fn is_meta_entry(item: &serde_json::Value) -> bool {
    item.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false)
}

/// Slash-command bookkeeping that Claude Code stores as user messages.
fn is_command_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<command-") || t.starts_with("<local-command")
}

/// First non-empty text of a message `content` (a string or a list of blocks).
fn first_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content? {
        serde_json::Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        serde_json::Value::Array(blocks) => blocks.iter().find_map(|block| {
            (block.get("type").and_then(|t| t.as_str()) == Some("text"))
                .then(|| block.get("text").and_then(|t| t.as_str()))
                .flatten()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
        }),
        _ => None,
    }
}

/// All text of a `content` value joined by newlines.
fn all_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn safe_truncate_tool_result(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}\n...[剩余内容已截断]", truncated)
    }
}

pub struct ClaudeRecoveryService<'a> {
    db: &'a Database,
    claude_home: PathBuf,
}

impl<'a> ClaudeRecoveryService<'a> {
    pub const SOURCE_TYPE: &'static str = "claude_code";
    pub const SOURCE_KIND: &'static str = "claude";

    pub fn new(db: &'a Database, custom_home: Option<&Path>) -> Self {
        let home = if let Some(p) = custom_home {
            p.to_path_buf()
        } else {
            // Priority: Portable ./data/claude -> env var CLAUDE_HOME -> %USERPROFILE%/.claude
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
        }
    }

    pub fn home_directory(&self) -> String {
        self.claude_home.to_string_lossy().to_string()
    }

    pub fn current_projects(&self) -> Vec<ClaudeProject> {
        let mut map: HashMap<String, String> = HashMap::new();

        // 1. From history.jsonl
        let history_map = self.load_history_index();
        for (_, (_, proj, _)) in history_map {
            let trimmed = proj.trim();
            if !trimmed.is_empty() {
                let name = Path::new(trimmed)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(trimmed)
                    .to_string();
                map.insert(trimmed.to_string(), name);
            }
        }

        // 2. From projects directory (first line peek)
        let projects_dir = self.claude_home.join("projects");
        if let Ok(entries) = fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                if let Ok(sess_entries) = fs::read_dir(&p) {
                    for se in sess_entries.flatten() {
                        let sp = se.path();
                        if sp.is_file()
                            && sp
                                .extension()
                                .and_then(|s| s.to_str())
                                .map(|s| s.eq_ignore_ascii_case("jsonl"))
                                .unwrap_or(false)
                        {
                            if let Some(c) = Self::read_session_cwd(&sp) {
                                let name = Path::new(&c)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(&c)
                                    .to_string();
                                map.insert(c, name);
                                break;
                            }
                        }
                    }
                }
            }
        }

        let mut projects: Vec<ClaudeProject> = map
            .into_iter()
            .map(|(path, name)| ClaudeProject {
                project_id: path.clone(),
                name,
                root_paths: vec![path],
            })
            .collect();

        projects.sort_by(|a, b| a.name.cmp(&b.name));
        projects
    }

    /// 扫描 Claude Code 本地历史项目与会话
    pub fn scan(&self) -> ClaudeRecoveryReport {
        let repo = Repository::new(self.db);
        let mut warnings = Vec::new();
        let mut candidates = Vec::new();

        if !self.claude_home.is_dir() {
            warnings.push(format!(
                "未在 {} 找到 Claude 配置目录",
                self.claude_home.display()
            ));
            return ClaudeRecoveryReport {
                candidates,
                total_scanned: 0,
                claude_home: self.home_directory(),
                excluded_count: 0,
                warnings,
            };
        }

        let projects_dir = self.claude_home.join("projects");
        if !projects_dir.is_dir() {
            warnings.push(format!(
                "未在 {} 找到 Claude 项目会话目录 (projects/)",
                self.claude_home.display()
            ));
            return ClaudeRecoveryReport {
                candidates,
                total_scanned: 0,
                claude_home: self.home_directory(),
                excluded_count: 0,
                warnings,
            };
        }

        // 读取 history.jsonl 建立历史索引 (sessionId -> (display, project, timestamp))
        let history_map = self.load_history_index();

        let excluded_ids = repo
            .excluded_source_ids(Self::SOURCE_TYPE)
            .unwrap_or_default();
        let imported_ids = repo
            .imported_source_ids(Self::SOURCE_TYPE)
            .unwrap_or_default();

        let mut excluded_count = 0;

        // 递归或分层遍历 projects/ 下各项目目录
        let project_entries = match fs::read_dir(&projects_dir) {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("读取 Claude projects 目录失败：{}", e));
                return ClaudeRecoveryReport {
                    candidates,
                    total_scanned: 0,
                    claude_home: self.home_directory(),
                    excluded_count: 0,
                    warnings,
                };
            }
        };

        for proj_entry in project_entries.flatten() {
            let proj_path = proj_entry.path();
            if !proj_path.is_dir() {
                continue;
            }

            // 读取该项目下的全部 *.jsonl 文件
            let session_entries = match fs::read_dir(&proj_path) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for sess_entry in session_entries.flatten() {
                let sess_path = sess_entry.path();
                if !sess_path.is_file() {
                    continue;
                }

                let ext = sess_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if ext != "jsonl" {
                    continue;
                }

                let session_id = sess_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if session_id.is_empty() {
                    continue;
                }

                // 检查是否已被用户删除排除
                if excluded_ids.contains(&session_id) {
                    excluded_count += 1;
                    continue;
                }

                // 获取文件最后修改时间
                let metadata = match fs::metadata(&sess_path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let last_modified_time = metadata
                    .modified()
                    .ok()
                    .map(|sys_time| chrono::DateTime::<chrono::Utc>::from(sys_time).to_rfc3339())
                    .unwrap_or_default();

                // 快速扫描会话获取元数据 (cwd, first user prompt, message count, title)
                let SessionPeek {
                    cwd,
                    first_prompt,
                    message_count,
                    custom_title: transcript_title,
                } = Self::peek_session_file(&sess_path);
                if message_count == 0 {
                    continue;
                }

                // custom-title.json (Claude desktop) takes precedence over transcript titles.
                let custom_title_path = proj_path.join(&session_id).join("custom-title.json");
                let custom_title = fs::read_to_string(&custom_title_path)
                    .ok()
                    .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
                    .and_then(|json| {
                        json.get("customTitle")
                            .and_then(|t| t.as_str())
                            .map(|s| s.trim().to_string())
                    })
                    .filter(|t| !t.is_empty())
                    .or(transcript_title);

                // 确定最终标题
                let final_title = custom_title.clone().unwrap_or_else(|| {
                    Self::resolve_title_fallback(&session_id, &history_map, &first_prompt)
                });
                let preview = if !first_prompt.is_empty() {
                    safe_truncate_chars(&first_prompt, 120)
                } else {
                    final_title.clone()
                };

                let exists_locally = imported_ids.contains(&session_id);

                candidates.push(ClaudeCandidate {
                    session_id,
                    title: final_title,
                    preview,
                    message_count,
                    last_modified_time,
                    project_path: cwd,
                    exists_locally,
                    session_file_path: sess_path.to_string_lossy().to_string(),
                    custom_title,
                });
            }
        }

        // 按修改时间倒序排列
        candidates.sort_by(|a, b| b.last_modified_time.cmp(&a.last_modified_time));

        let total = candidates.len();
        ClaudeRecoveryReport {
            candidates,
            total_scanned: total,
            claude_home: self.home_directory(),
            excluded_count,
            warnings,
        }
    }

    /// 自动向本地数据库同步已存在会话的外部最新标题
    pub fn sync_existing_titles(&self, candidates: &[ClaudeCandidate]) -> usize {
        let repo = Repository::new(self.db);
        let mut titles = HashMap::new();
        for c in candidates {
            if c.exists_locally {
                titles.insert(c.session_id.clone(), c.title.clone());
            }
        }
        repo.sync_import_titles(Self::SOURCE_TYPE, &titles)
            .unwrap_or(0)
    }

    /// 执行选定候选会话的恢复与导入
    pub fn recover(&self, candidates: &[ClaudeCandidate]) -> ClaudeRecoveryResult {
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
            if excluded_ids.contains(&candidate.session_id) {
                skipped += 1;
                warnings.push(format!("“{}”已被排除，跳过恢复", candidate.title));
                continue;
            }

            let path = Path::new(&candidate.session_file_path);
            let imported_messages = Self::parse_session_file(path, &candidate.session_id);
            if imported_messages.is_empty() {
                skipped += 1;
                warnings.push(format!("“{}”没有可解析的有效对话消息", candidate.title));
                continue;
            }

            let active_ext_id = imported_messages.last().map(|m| m.external_id.as_str());

            match repo.sync_import_conversation(
                &candidate.title,
                Self::SOURCE_TYPE,
                &candidate.session_id,
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

        ClaudeRecoveryResult {
            conversation_count: conversations,
            new_conversation_count: new_conversations,
            message_count: messages,
            title_update_count: title_updates,
            skipped_count: skipped,
            warnings,
        }
    }

    /// Reads only up to the first `cwd` field, which is all the project list needs.
    fn read_session_cwd(path: &Path) -> Option<String> {
        let reader = BufReader::new(File::open(path).ok()?);
        for line in reader.lines().take(200).flatten() {
            if !line.contains("\"cwd\"") {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(c) = v.get("cwd").and_then(|s| s.as_str()) {
                    if !c.trim().is_empty() {
                        return Some(c.trim().to_string());
                    }
                }
            }
        }
        None
    }

    /// 快速预览文件获取基本信息
    fn peek_session_file(path: &Path) -> SessionPeek {
        let mut peek = SessionPeek::default();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return peek,
        };

        for line in BufReader::new(file).lines().flatten() {
            let v: serde_json::Value = match serde_json::from_str(&line) {
                Ok(val) => val,
                Err(_) => continue,
            };

            if peek.cwd.is_none() {
                if let Some(c) = v.get("cwd").and_then(|s| s.as_str()) {
                    if !c.trim().is_empty() {
                        peek.cwd = Some(c.trim().to_string());
                    }
                }
            }

            match v.get("type").and_then(|s| s.as_str()).unwrap_or("") {
                "user" => {
                    peek.message_count += 1;
                    if peek.first_prompt.is_empty() && !is_meta_entry(&v) {
                        if let Some(text) = v.get("message").and_then(|m| first_text(m.get("content"))) {
                            if !is_command_text(&text) {
                                peek.first_prompt = text;
                            }
                        }
                    }
                }
                "assistant" => peek.message_count += 1,
                // Claude Code records /rename and generated titles as transcript lines;
                // the newest one is the title it displays.
                "custom-title" => {
                    if let Some(t) = v.get("customTitle").and_then(|t| t.as_str()) {
                        if !t.trim().is_empty() {
                            peek.custom_title = Some(t.trim().to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        peek
    }

    /// 解析完整会话文件中的消息流
    pub fn parse_session_file(path: &Path, session_id: &str) -> Vec<ImportedMessagePayload> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut messages: Vec<ImportedMessagePayload> = Vec::new();
        let mut prev_external_id: Option<String> = None;

        // 用于合并连续 Assistant 的同一次请求块 (相同 requestId)
        let mut current_assistant_req_id: Option<String> = None;
        let mut current_assistant_uuid: Option<String> = None;
        let mut current_assistant_time: Option<String> = None;
        let mut current_assistant_parts: Vec<String> = Vec::new();

        let flush_assistant = |req_id: &mut Option<String>,
                               asst_uuid: &mut Option<String>,
                               asst_time: &mut Option<String>,
                               parts: &mut Vec<String>,
                               messages: &mut Vec<ImportedMessagePayload>,
                               prev_id: &mut Option<String>| {
            if parts.is_empty() {
                return;
            }
            let full_text = parts.join("\n\n").trim().to_string();
            if !full_text.is_empty() {
                let ext_id = asst_uuid.clone().unwrap_or_else(|| {
                    req_id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
                });
                let payload = ImportedMessagePayload {
                    role: "assistant".to_string(),
                    content: full_text,
                    external_id: ext_id.clone(),
                    parent_external_id: prev_id.clone(),
                    created_at: asst_time.clone(),
                };
                messages.push(payload);
                *prev_id = Some(ext_id);
            }
            parts.clear();
            *req_id = None;
            *asst_uuid = None;
            *asst_time = None;
        };

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(_) => continue,
            };

            let item: serde_json::Value = match serde_json::from_str(&line) {
                Ok(val) => val,
                Err(_) => continue,
            };

            let typ = item.get("type").and_then(|s| s.as_str()).unwrap_or("");
            let item_uuid = item
                .get("uuid")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{}:{}", session_id, line_idx));
            let created_at = item
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            if typ == "user" && is_meta_entry(&item) {
                // Injected context such as local-command caveats is not part of the dialogue.
                continue;
            } else if typ == "user" {
                // 如果之前有待提交的 assistant 响应，先 flush
                flush_assistant(
                    &mut current_assistant_req_id,
                    &mut current_assistant_uuid,
                    &mut current_assistant_time,
                    &mut current_assistant_parts,
                    &mut messages,
                    &mut prev_external_id,
                );

                if let Some(msg) = item.get("message") {
                    let mut text_parts = Vec::new();

                    if let Some(content_str) = msg.get("content").and_then(|c| c.as_str()) {
                        let cleaned = content_str.trim().to_string();
                        if !cleaned.is_empty() {
                            text_parts.push(cleaned);
                        }
                    } else if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in arr {
                            let block_type =
                                block.get("type").and_then(|s| s.as_str()).unwrap_or("");
                            if block_type == "text" {
                                if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                                    let cleaned = txt.trim().to_string();
                                    if !cleaned.is_empty() {
                                        text_parts.push(cleaned);
                                    }
                                }
                            } else if block_type == "tool_result" {
                                // Tool output is either a string or a list of text blocks.
                                let tr_text = all_text(block.get("content"));
                                if !tr_text.is_empty() {
                                    // 截断过长的工具返回值，保持会话流流畅
                                    let display_res = safe_truncate_tool_result(&tr_text, 2000);
                                    text_parts.push(format!(
                                        "📋 **工具返回结果:**\n```text\n{}\n```",
                                        display_res
                                    ));
                                }
                            }
                        }
                    }

                    if !text_parts.is_empty() {
                        let full_content = text_parts.join("\n\n");
                        let payload = ImportedMessagePayload {
                            role: "user".to_string(),
                            content: full_content,
                            external_id: item_uuid.clone(),
                            parent_external_id: prev_external_id.clone(),
                            created_at,
                        };
                        messages.push(payload);
                        prev_external_id = Some(item_uuid);
                    }
                }
            } else if typ == "assistant" {
                let req_id = item
                    .get("requestId")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());

                // 若换了一个 requestId 且不是连续请求，先 flush 上一个
                if let (Some(cur_req), Some(new_req)) = (&current_assistant_req_id, &req_id) {
                    if cur_req != new_req {
                        flush_assistant(
                            &mut current_assistant_req_id,
                            &mut current_assistant_uuid,
                            &mut current_assistant_time,
                            &mut current_assistant_parts,
                            &mut messages,
                            &mut prev_external_id,
                        );
                    }
                }

                if current_assistant_req_id.is_none() {
                    current_assistant_req_id = req_id;
                    current_assistant_uuid = Some(item_uuid);
                    current_assistant_time = created_at;
                }

                if let Some(msg) = item.get("message") {
                    if let Some(content_str) = msg.get("content").and_then(|c| c.as_str()) {
                        let cleaned = content_str.trim().to_string();
                        if !cleaned.is_empty() {
                            current_assistant_parts.push(cleaned);
                        }
                    } else if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in arr {
                            let block_type =
                                block.get("type").and_then(|s| s.as_str()).unwrap_or("");
                            if block_type == "text" {
                                if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                                    let cleaned = txt.trim().to_string();
                                    if !cleaned.is_empty() {
                                        current_assistant_parts.push(cleaned);
                                    }
                                }
                            } else if block_type == "thinking" {
                                if let Some(thk) = block.get("thinking").and_then(|t| t.as_str()) {
                                    let cleaned = thk.trim();
                                    if !cleaned.is_empty() {
                                        current_assistant_parts.push(format!(
                                            "<details>\n<summary>💭 <b>深度思考过程</b></summary>\n\n{}\n</details>",
                                            cleaned
                                        ));
                                    }
                                }
                            } else if block_type == "tool_use" {
                                let name =
                                    block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                                let input_str = if let Some(inp) = block.get("input") {
                                    if let Some(cmd) = inp.get("command").and_then(|c| c.as_str()) {
                                        cmd.trim().to_string()
                                    } else {
                                        serde_json::to_string_pretty(inp).unwrap_or_default()
                                    }
                                } else {
                                    String::new()
                                };
                                current_assistant_parts.push(format!(
                                    "🔧 **调用工具: `{}`**\n```bash\n{}\n```",
                                    name, input_str
                                ));
                            }
                        }
                    }
                }
            }
        }

        // flush 尾部可能遗留的 assistant 响应
        flush_assistant(
            &mut current_assistant_req_id,
            &mut current_assistant_uuid,
            &mut current_assistant_time,
            &mut current_assistant_parts,
            &mut messages,
            &mut prev_external_id,
        );

        messages
    }

    /// 从 history.jsonl 加载历史索引
    fn load_history_index(&self) -> HashMap<String, (String, String, i64)> {
        let mut map = HashMap::new();
        let history_file = self.claude_home.join("history.jsonl");
        if !history_file.is_file() {
            return map;
        }

        if let Ok(file) = File::open(&history_file) {
            let reader = BufReader::new(file);
            for line_res in reader.lines() {
                if let Ok(line) = line_res {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        let sess_id = json
                            .get("sessionId")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !sess_id.is_empty() {
                            let display = json
                                .get("display")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string();
                            let project = json
                                .get("project")
                                .and_then(|p| p.as_str())
                                .unwrap_or("")
                                .to_string();
                            let timestamp =
                                json.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0);
                            map.entry(sess_id).or_insert((display, project, timestamp));
                        }
                    }
                }
            }
        }
        map
    }

    fn resolve_title_fallback(
        session_id: &str,
        history_map: &HashMap<String, (String, String, i64)>,
        first_prompt: &str,
    ) -> String {
        if let Some((disp, _, _)) = history_map.get(session_id) {
            let trimmed = disp.trim();
            if !trimmed.is_empty() {
                return safe_truncate_chars(trimmed, 60);
            }
        }

        let p = first_prompt.trim();
        if !p.is_empty() {
            safe_truncate_chars(p, 60)
        } else {
            "Claude 会话".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_claude_jsonl() {
        let sample_jsonl = r#"{"type":"custom-title","customTitle":"Test Session Title","sessionId":"sess-123"}
{"type":"user","message":{"role":"user","content":"How do I optimize casting mold?"},"uuid":"u-1","timestamp":"2026-09-29T06:00:00Z","cwd":"C:\\demo"}
{"type":"assistant","requestId":"req-1","uuid":"a-1","timestamp":"2026-09-29T06:00:01Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Analyzing thermal parameters..."},{"type":"text","text":"You can adjust the cooling water flow rate."}]}}
"#;
        let file_path =
            std::env::temp_dir().join(format!("test_claude_sess_{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(&file_path, sample_jsonl).unwrap();

        let msgs = ClaudeRecoveryService::parse_session_file(&file_path, "sess-123");
        let _ = std::fs::remove_file(&file_path);

        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "How do I optimize casting mold?");
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("深度思考过程"));
        assert!(msgs[1]
            .content
            .contains("You can adjust the cooling water flow rate."));
    }

    #[test]
    #[ignore = "Manual integration test requiring local Claude history"]
    fn test_real_claude_scan_and_recover() {
        let claude_home = dirs::home_dir().map(|h| h.join(".claude"));
        if let Some(home) = claude_home {
            if home.is_dir() {
                let temp_dir =
                    std::env::temp_dir().join(format!("test_claude_db_{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&temp_dir).unwrap();
                let db_path = temp_dir.join("test.db");
                let backup_path = temp_dir.join("backups");
                let db = Database::new(&db_path, &backup_path).unwrap();

                let service = ClaudeRecoveryService::new(&db, Some(&home));
                let report = service.scan();
                println!("Scanned candidates count: {}", report.candidates.len());
                for c in &report.candidates {
                    println!(
                        "Candidate: {} (sess: {}, msgs: {}, path: {})",
                        c.title, c.session_id, c.message_count, c.session_file_path
                    );
                }

                if !report.candidates.is_empty() {
                    let result = service.recover(&report.candidates);
                    println!(
                        "Recover result: new_convs={}, msgs={}, warnings={:?}",
                        result.new_conversation_count, result.message_count, result.warnings
                    );
                    assert!(result.new_conversation_count > 0 || result.conversation_count > 0);
                }
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
        }
    }
}
