use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::persistence::{Database, ImportedMessagePayload, Repository};
use crate::util::{first_non_empty, normalize_timestamp, text_from_value, timestamp_from_value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexProject {
    pub project_id: String,
    pub name: String,
    pub root_paths: Vec<String>,
}

impl CodexProject {
    pub fn primary_path(&self) -> &str {
        self.root_paths.first().map(|s| s.as_str()).unwrap_or("")
    }
}

/// Where the Codex desktop sidebar lists a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexScope {
    /// Under one of the sidebar's projects.
    Project,
    /// Among the chats that belong to no project.
    Chat,
    /// Only kept in Codex's databases; the sidebar does not list it.
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexPlacement {
    pub scope: CodexScope,
    /// The local project for `CodexScope::Project`.
    pub project_id: Option<String>,
}

/// What the Codex sidebar lists, so the Codex tab can be grouped the same way.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexVisibility {
    /// False when Codex keeps no project state; the tab then groups by folder.
    pub available: bool,
    /// The sidebar's projects, in sidebar order.
    pub projects: Vec<CodexProject>,
    /// Vault conversation ID → placement, for the conversations Codex lists.
    /// Conversations left out are hidden in Codex.
    pub conversations: HashMap<String, CodexPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexThreadCandidate {
    pub thread_id: String,
    pub title: String,
    pub rollout_path: String,
    pub project_path: Option<String>,
    pub exists_locally: bool,
    pub model: Option<String>,
    pub observed_providers: Vec<String>,
    pub updated_at: String,
    pub archived: bool,
    pub source_indexes: Vec<String>,
    /// The rollout file is gone; messages come from `thread_history_N.sqlite`, whose
    /// path is then stored in `rollout_path`.
    #[serde(default)]
    pub history_only: bool,
    /// Where the Codex sidebar lists the thread; `None` when Codex keeps no project state.
    #[serde(default)]
    pub codex_scope: Option<CodexScope>,
    #[serde(default)]
    pub codex_project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRecoveryReport {
    pub candidates: Vec<CodexThreadCandidate>,
    pub index_count: usize,
    pub missing_rollouts: usize,
    pub excluded_count: usize,
    pub warnings: Vec<String>,
    /// The Codex sidebar's projects, in sidebar order; `None` when Codex keeps no
    /// project state.
    #[serde(default)]
    pub sidebar_projects: Option<Vec<CodexProject>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRecoveryResult {
    pub conversation_count: usize,
    pub new_conversation_count: usize,
    pub message_count: usize,
    pub title_update_count: usize,
    pub skipped_count: usize,
    pub warnings: Vec<String>,
}

pub struct CodexRecoveryService<'a> {
    db: &'a Database,
    codex_home: PathBuf,
}

impl<'a> CodexRecoveryService<'a> {
    pub const SOURCE_TYPE: &'static str = "codex_rollout";

    pub fn new(db: &'a Database, custom_codex_home: Option<&Path>) -> Self {
        let home = if let Some(p) = custom_codex_home {
            p.to_path_buf()
        } else {
            dirs::home_dir()
                .map(|h| h.join(".codex"))
                .unwrap_or_else(|| PathBuf::from(".codex"))
        };
        Self {
            db,
            codex_home: home,
        }
    }

    pub fn current_projects(&self) -> Vec<CodexProject> {
        self.read_sidebar_state().projects
    }

    /// Where Codex currently lists each conversation imported from it.
    pub fn visibility(&self) -> Result<CodexVisibility, String> {
        let sidebar = self.read_sidebar_state();
        if !sidebar.available {
            return Ok(CodexVisibility {
                available: false,
                projects: Vec::new(),
                conversations: HashMap::new(),
            });
        }
        let archived = self.archived_thread_ids();
        let threads = Repository::new(self.db)
            .source_ids_by_conversation(Self::SOURCE_TYPE)
            .map_err(|e| e.to_string())?;
        let conversations = threads
            .into_iter()
            .filter_map(|(conversation_id, thread_id)| {
                let placement = sidebar.placement(&thread_id, &archived);
                (placement.scope != CodexScope::Hidden).then_some((conversation_id, placement))
            })
            .collect();
        Ok(CodexVisibility {
            available: true,
            projects: sidebar.projects,
            conversations,
        })
    }

    pub fn scan(&self) -> CodexRecoveryReport {
        let repo = Repository::new(self.db);
        let sidebar = self.read_sidebar_state();
        let archived_ids = self.archived_thread_ids();
        let rollout_by_id = self.rollout_index();
        let mut merged: HashMap<String, MergedCandidate> = HashMap::new();
        let mut warnings = Vec::new();

        let mut sources: Vec<(String, PathBuf)> = Vec::new();
        let catalog_path = self.codex_home.join("sqlite").join("codex-dev.db");
        // The catalog caches every thread Codex has seen, not just the ones its
        // sidebar lists.
        if catalog_path.is_file() {
            sources.push(("Codex 线程缓存".to_string(), catalog_path.clone()));
        }
        for (lbl, p) in self.state_index_paths() {
            sources.push((lbl, p));
        }

        for (label, path) in &sources {
            let rows_res = if *path == catalog_path {
                Self::read_catalog_index(path)
            } else {
                Self::read_thread_index(path, false)
            };

            match rows_res {
                Ok(rows) => {
                    for row in rows {
                        let thread_id = row.id.clone();
                        let row_title = clean_title(&row.title, &thread_id);
                        let project_path =
                            resolve_project_path(&thread_id, row.cwd.as_deref(), &sidebar);

                        let entry = merged.entry(thread_id.clone()).or_insert_with(|| MergedCandidate {
                            thread_id: thread_id.clone(),
                            title: row_title.clone(),
                            rollout_path: None,
                            project_path: project_path.clone(),
                            model: row.model.clone(),
                            providers: HashSet::new(),
                            updated_at: normalize_recency(&row.updated_at),
                            archived: row.archived,
                            sources: HashSet::new(),
                        });

                        entry.sources.insert(label.clone());
                        if let Some(ref prov) = row.model_provider {
                            entry.providers.insert(prov.clone());
                        }
                        if entry.model.is_none() && row.model.is_some() {
                            entry.model = row.model.clone();
                        }
                        if entry.title == "Codex 会话" && row_title != "Codex 会话" {
                            entry.title = row_title;
                        }
                        if entry.project_path.is_none() && project_path.is_some() {
                            entry.project_path = project_path;
                        }
                        if entry.rollout_path.is_none() {
                            if let Some(ref cand_path) = row.rollout_path {
                                let p = PathBuf::from(cand_path);
                                if p.is_file() {
                                    entry.rollout_path = Some(p);
                                }
                            }
                            if entry.rollout_path.is_none() {
                                if let Some(p) = rollout_by_id.get(&thread_id) {
                                    entry.rollout_path = Some(p.clone());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warnings.push(format!("无法读取 {}：{}", label, e));
                }
            }
        }

        // Rollout fallback
        for (thread_id, rollout_path) in &rollout_by_id {
            if merged.contains_key(thread_id) {
                continue;
            }
            if let Some(meta) = read_rollout_metadata(rollout_path, thread_id) {
                let project_path = resolve_project_path(thread_id, meta.cwd.as_deref(), &sidebar);
                let mut provs = HashSet::new();
                if let Some(p) = meta.model_provider {
                    provs.insert(p);
                }
                let mut srcs = HashSet::new();
                srcs.insert("rollout 文件兜底".to_string());

                merged.insert(
                    thread_id.clone(),
                    MergedCandidate {
                        thread_id: thread_id.clone(),
                        title: meta.title,
                        rollout_path: Some(rollout_path.clone()),
                        project_path,
                        model: meta.model,
                        providers: provs,
                        updated_at: meta.updated_at,
                        archived: meta.archived,
                        sources: srcs,
                    },
                );
            }
        }

        let excluded_ids = repo.excluded_source_ids(Self::SOURCE_TYPE).unwrap_or_default();
        let imported_ids = repo.imported_source_ids(Self::SOURCE_TYPE).unwrap_or_default();

        let history_path = self.history_db_path();
        let history = history_path
            .as_ref()
            .and_then(|p| Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).ok());

        let mut candidates = Vec::new();
        let mut missing = 0;
        let mut excluded = 0;

        for item in merged.into_values() {
            if excluded_ids.contains(&item.thread_id) {
                excluded += 1;
                continue;
            }
            let r_path = item
                .rollout_path
                .or_else(|| rollout_by_id.get(&item.thread_id).cloned())
                .filter(|p| p.is_file());
            let (rollout_path_buf, history_only) = match (r_path, &history, &history_path) {
                (Some(p), _, _) => (p, false),
                (None, Some(conn), Some(db_path))
                    if has_history_messages(conn, &item.thread_id) =>
                {
                    (db_path.clone(), true)
                }
                _ => {
                    missing += 1;
                    continue;
                }
            };

            let mut sorted_provs: Vec<String> = item.providers.into_iter().collect();
            sorted_provs.sort();
            let mut sorted_srcs: Vec<String> = item.sources.into_iter().collect();
            sorted_srcs.sort();
            let placement = sidebar
                .available
                .then(|| sidebar.placement(&item.thread_id, &archived_ids));

            candidates.push(CodexThreadCandidate {
                thread_id: item.thread_id.clone(),
                title: if item.title.trim().is_empty() {
                    "Codex 会话".to_string()
                } else {
                    item.title
                },
                rollout_path: rollout_path_buf.to_string_lossy().to_string(),
                project_path: item.project_path,
                exists_locally: imported_ids.contains(&item.thread_id),
                model: item.model,
                observed_providers: sorted_provs,
                updated_at: item.updated_at,
                archived: item.archived,
                source_indexes: sorted_srcs,
                history_only,
                codex_scope: placement.as_ref().map(|p| p.scope),
                codex_project_id: placement.and_then(|p| p.project_id),
            });
        }

        candidates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        CodexRecoveryReport {
            candidates,
            index_count: sources.len(),
            missing_rollouts: missing,
            excluded_count: excluded,
            warnings,
            sidebar_projects: sidebar.available.then_some(sidebar.projects),
        }
    }

    pub fn sync_existing_titles(&self, candidates: &[CodexThreadCandidate]) -> usize {
        let repo = Repository::new(self.db);
        let mut titles = HashMap::new();
        for c in candidates {
            if c.exists_locally {
                titles.insert(c.thread_id.clone(), c.title.clone());
            }
        }
        repo.sync_import_titles(Self::SOURCE_TYPE, &titles).unwrap_or(0)
    }

    pub fn recover(
        &self,
        candidates: &[CodexThreadCandidate],
    ) -> CodexRecoveryResult {
        let repo = Repository::new(self.db);
        let excluded_ids = repo.excluded_source_ids(Self::SOURCE_TYPE).unwrap_or_default();

        let mut conversations = 0;
        let mut new_conversations = 0;
        let mut messages = 0;
        let mut title_updates = 0;
        let mut skipped = 0;
        let mut warnings = Vec::new();

        for candidate in candidates {
            if excluded_ids.contains(&candidate.thread_id) {
                skipped += 1;
                warnings.push(format!("“{}”已被用户删除，恢复已跳过", candidate.title));
                continue;
            }

            let path = Path::new(&candidate.rollout_path);
            let imported_messages = if candidate.history_only {
                // Message IDs differ between rollouts and the history database, so a
                // conversation recovered from a since-deleted rollout is left as is.
                if candidate.exists_locally
                    && !repo
                        .has_source_message_prefix(
                            Self::SOURCE_TYPE,
                            &candidate.thread_id,
                            HISTORY_ID_PREFIX,
                        )
                        .unwrap_or(false)
                {
                    skipped += 1;
                    warnings.push(format!(
                        "“{}”的 rollout 文件已不存在，保留已恢复的内容",
                        candidate.title
                    ));
                    continue;
                }
                Self::parse_history(path, &candidate.thread_id)
            } else {
                Self::parse_rollout(path)
            };
            if imported_messages.is_empty() {
                skipped += 1;
                warnings.push(format!("“{}”没有可恢复的用户/助手文本", candidate.title));
                continue;
            }

            let active_ext_id = imported_messages.last().map(|m| m.external_id.as_str());

            match repo.sync_import_conversation(
                &candidate.title,
                Self::SOURCE_TYPE,
                &candidate.thread_id,
                &imported_messages,
                active_ext_id,
                candidate.project_path.as_deref(),
                "codex_main",
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

        CodexRecoveryResult {
            conversation_count: conversations,
            new_conversation_count: new_conversations,
            message_count: messages,
            title_update_count: title_updates,
            skipped_count: skipped,
            warnings,
        }
    }

    pub fn parse_rollout(path: &Path) -> Vec<ImportedMessagePayload> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let mut event_rows: Vec<EventRow> = Vec::new();
        let mut response_messages: Vec<ImportedMessagePayload> = Vec::new();
        let mut response_parent: Option<String> = None;
        let mut has_final_event = false;

        // One reused buffer; message IDs use line numbers, so every line is counted.
        let mut reader = BufReader::with_capacity(1 << 20, file);
        let mut line = Vec::new();
        let mut line_num = 0;
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => line_num += 1,
                Err(_) => break,
            }
            if !rollout_line_may_hold_message(&line) {
                continue;
            }

            let item: serde_json::Value = match serde_json::from_slice(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let payload = match item.get("payload") {
                Some(p) if p.is_object() => p,
                _ => continue,
            };

            let timestamp = item.get("timestamp").and_then(|t| t.as_str()).map(|s| s.to_string());
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            if item_type == "event_msg" {
                let event_type = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");
                let role = if event_type == "user_message" {
                    "user"
                } else if event_type == "agent_message" {
                    let phase = payload.get("phase").and_then(|p| p.as_str()).unwrap_or("");
                    if phase == "" || phase == "final_answer" {
                        has_final_event = true;
                    }
                    "assistant"
                } else {
                    continue;
                };

                let content = payload.get("message").and_then(|m| m.as_str()).unwrap_or("").trim().to_string();
                if content.is_empty() {
                    continue;
                }

                let ext_id = payload.get("client_id")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("event:{}", line_num));

                event_rows.push(EventRow {
                    role: role.to_string(),
                    content,
                    external_id: ext_id,
                    created_at: timestamp,
                });
            } else if item_type == "response_item" && payload.get("type").and_then(|t| t.as_str()) == Some("message") {
                let role = payload.get("role").and_then(|r| r.as_str()).unwrap_or("");
                if role != "user" && role != "assistant" {
                    continue;
                }
                let content = extract_response_text(payload.get("content")).trim().to_string();
                if content.is_empty() {
                    continue;
                }
                let ext_id = payload.get("id")
                    .and_then(|i| i.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("response:{}", line_num));

                response_messages.push(ImportedMessagePayload {
                    role: role.to_string(),
                    content,
                    external_id: ext_id.clone(),
                    parent_external_id: response_parent,
                    created_at: timestamp,
                });
                response_parent = Some(ext_id);
            }
        }

        let visible_events = select_visible_events(event_rows);
        if !response_messages.is_empty()
            && !has_final_event
            && response_messages.iter().any(|m| m.role == "assistant")
        {
            return response_messages;
        }

        if !visible_events.is_empty() {
            visible_events
        } else {
            response_messages
        }
    }

    /// Newer Codex builds keep paginated thread history in `thread_history_<N>.sqlite`.
    fn history_db_path(&self) -> Option<PathBuf> {
        fs::read_dir(&self.codex_home)
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let version: u32 = name
                    .strip_prefix("thread_history_")?
                    .strip_suffix(".sqlite")?
                    .parse()
                    .ok()?;
                Some((version, entry.path()))
            })
            .max_by_key(|(version, _)| *version)
            .map(|(_, path)| path)
    }

    /// Reads user and agent messages of one thread from the history database.
    pub fn parse_history(path: &Path, thread_id: &str) -> Vec<ImportedMessagePayload> {
        let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            return Vec::new();
        };
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        let Ok(mut stmt) = conn.prepare(
            "SELECT item_type, item_json, created_at_ms FROM thread_items
             WHERE thread_id = ?1 AND item_type IN ('userMessage', 'agentMessage')
             ORDER BY rollout_ordinal",
        ) else {
            return Vec::new();
        };
        let rows = match stmt.query_map([thread_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };

        let mut messages = Vec::new();
        let mut parent: Option<String> = None;
        for (item_type, json, created_ms) in rows.flatten() {
            let Ok(item) = serde_json::from_str::<serde_json::Value>(&json) else {
                continue;
            };
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let (role, content) = if item_type == "userMessage" {
                let parts: Vec<String> = item
                    .get("content")
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .filter_map(user_request_text)
                            .collect()
                    })
                    .unwrap_or_default();
                ("user", parts.join("\n\n"))
            } else {
                let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                ("assistant", text.trim().to_string())
            };
            if id.is_empty() || content.is_empty() {
                continue;
            }
            let external_id = format!("{}{}", HISTORY_ID_PREFIX, id);
            messages.push(ImportedMessagePayload {
                role: role.to_string(),
                content,
                external_id: external_id.clone(),
                parent_external_id: parent.replace(external_id),
                created_at: created_ms.and_then(|ms| crate::util::epoch_to_rfc3339(ms as f64)),
            });
        }
        messages
    }

    /// Reads what the Codex desktop sidebar lists from `.codex-global-state.json`:
    /// its projects, the threads assigned to them, and the chats outside any project.
    fn read_sidebar_state(&self) -> SidebarState {
        let path = self.codex_home.join(".codex-global-state.json");
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return SidebarState::default(),
        };

        let payload: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return SidebarState::default(),
        };

        let mut projects_by_id = HashMap::new();
        if let Some(raw_projects) = payload.get("local-projects").and_then(|p| p.as_object()) {
            for (project_id, raw_proj) in raw_projects {
                if let Some(roots) = raw_proj.get("rootPaths").and_then(|r| r.as_array()) {
                    let root_paths: Vec<String> = roots.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if root_paths.is_empty() {
                        continue;
                    }
                    let name = raw_proj.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            Path::new(&root_paths[0])
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string())
                                .unwrap_or_default()
                        });
                    projects_by_id.insert(
                        project_id.clone(),
                        CodexProject {
                            project_id: project_id.clone(),
                            name,
                            root_paths,
                        },
                    );
                }
            }
        }

        let mut ordered_ids = Vec::new();
        if let Some(raw_order) = payload.get("project-order").and_then(|o| o.as_array()) {
            for v in raw_order {
                if let Some(s) = v.as_str() {
                    ordered_ids.push(s.to_string());
                }
            }
        }
        for pid in projects_by_id.keys() {
            if !ordered_ids.contains(pid) {
                ordered_ids.push(pid.clone());
            }
        }

        let mut projects = Vec::new();
        for pid in ordered_ids {
            if let Some(proj) = projects_by_id.remove(&pid) {
                projects.push(proj);
            }
        }

        let mut assignments = HashMap::new();
        if let Some(raw_assign) = payload.get("thread-project-assignments").and_then(|a| a.as_object()) {
            for (tid, assign) in raw_assign {
                if assign.get("projectKind").and_then(|k| k.as_str()) == Some("local") {
                    if let Some(pid) = assign.get("projectId").and_then(|p| p.as_str()) {
                        assignments.insert(tid.clone(), pid.to_string());
                    }
                }
            }
        }

        let chats = payload
            .get("projectless-thread-ids")
            .and_then(|c| c.as_array())
            .map(|ids| ids.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        SidebarState {
            // Older Codex builds list workspace folders instead of projects.
            available: payload.get("local-projects").is_some(),
            projects,
            assignments,
            chats,
        }
    }

    /// Threads Codex has archived; its sidebar leaves them out.
    fn archived_thread_ids(&self) -> HashSet<String> {
        let path = self.codex_home.join("state_5.sqlite");
        if !path.is_file() {
            return HashSet::new();
        }
        let read = || -> rusqlite::Result<HashSet<String>> {
            let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let mut stmt = conn.prepare("SELECT id FROM threads WHERE archived = 1")?;
            let ids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(ids)
        };
        read().unwrap_or_default()
    }

    fn rollout_index(&self) -> HashMap<String, PathBuf> {
        let mut ranked: HashMap<String, (i32, u64, PathBuf)> = HashMap::new();
        let re = Regex::new(r"([0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12})$").unwrap();

        for (prio, root) in [
            (2, self.codex_home.join("sessions")),
            (1, self.codex_home.join("archived_sessions")),
        ] {
            if !root.is_dir() {
                continue;
            }
            Self::traverse_rollouts(&root, prio, &re, &mut ranked);
        }

        ranked.into_iter().map(|(k, (_, _, p))| (k, p)).collect()
    }

    fn traverse_rollouts(
        dir: &Path,
        prio: i32,
        re: &Regex,
        ranked: &mut HashMap<String, (i32, u64, PathBuf)>,
    ) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    Self::traverse_rollouts(&p, prio, re, ranked);
                } else if p.is_file() {
                    let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if file_name.starts_with("rollout-") && file_name.ends_with(".jsonl") {
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        if let Some(caps) = re.captures(stem) {
                            let thread_id = caps.get(1).unwrap().as_str().to_string();
                            let mtime = entry.metadata()
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs())
                                .unwrap_or(0);

                            let candidate = (prio, mtime, p);
                            let replace = match ranked.get(&thread_id) {
                                Some(existing) => (candidate.0, candidate.1) > (existing.0, existing.1),
                                None => true,
                            };
                            if replace {
                                ranked.insert(thread_id, candidate);
                            }
                        }
                    }
                }
            }
        }
    }

    fn state_index_paths(&self) -> Vec<(String, PathBuf)> {
        let mut paths = Vec::new();
        let current = self.codex_home.join("state_5.sqlite");
        if current.is_file() {
            paths.push(("当前 Codex".to_string(), current));
        }

        let backup_root = self.codex_home.join("backups_state").join("provider-sync");
        if let Ok(entries) = fs::read_dir(&backup_root) {
            for entry in entries.flatten() {
                let db_path = entry.path().join("db").join("state_5.sqlite");
                if db_path.is_file() {
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    paths.push((format!("切换备份 {}", folder_name), db_path));
                }
            }
        }
        paths
    }

    fn read_thread_index(path: &Path, include_subagents: bool) -> rusqlite::Result<Vec<ThreadIndexRow>> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut cols = HashSet::new();
        let mut stmt = conn.prepare("PRAGMA table_info(threads)")?;
        let col_iter = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for c in col_iter.flatten() {
            cols.insert(c);
        }

        let mut conditions = Vec::new();
        if cols.contains("has_user_event") {
            conditions.push("has_user_event = 1".to_string());
        }
        if !include_subagents {
            if cols.contains("thread_source") {
                conditions.push("COALESCE(thread_source, '') <> 'subagent'".to_string());
            }
            if cols.contains("source") {
                conditions.push("source NOT LIKE '{\"subagent\"%'".to_string());
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let col_sql = |name: &str| -> String {
            if cols.contains(name) {
                name.to_string()
            } else {
                format!("NULL AS {}", name)
            }
        };

        let sql = format!(
            "SELECT id, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}
             FROM threads {}",
            col_sql("rollout_path"),
            col_sql("updated_at"),
            col_sql("updated_at_ms"),
            col_sql("model_provider"),
            col_sql("title"),
            col_sql("first_user_message"),
            col_sql("name"),
            col_sql("archived"),
            col_sql("model"),
            col_sql("cwd"),
            where_clause
        );

        // Codex stores times as INTEGER epoch values; older builds used other storage
        // classes, so every column is read as a dynamic value.
        let mut qstmt = conn.prepare(&sql)?;
        let rows = qstmt.query_map([], |row| {
            let text = |idx: usize| -> rusqlite::Result<Option<String>> {
                Ok(text_from_value(&row.get::<_, Value>(idx)?))
            };
            let id = text(0)?.unwrap_or_default();
            let updated_at = timestamp_from_value(&row.get::<_, Value>(3)?)
                .or(timestamp_from_value(&row.get::<_, Value>(2)?))
                .unwrap_or_default();
            let archived = match row.get::<_, Value>(8)? {
                Value::Integer(i) => i != 0,
                Value::Text(s) => s == "1" || s.eq_ignore_ascii_case("true"),
                _ => false,
            };

            Ok(ThreadIndexRow {
                id,
                rollout_path: text(1)?,
                updated_at,
                model_provider: text(4)?,
                title: first_non_empty([text(7)?, text(5)?, text(6)?])
                    .unwrap_or_else(|| "Codex 会话".to_string()),
                archived,
                model: text(9)?,
                cwd: text(10)?,
            })
        })?;

        Ok(rows
            .filter_map(|r| r.ok())
            .filter(|r| !r.id.is_empty())
            .collect())
    }

    fn read_catalog_index(path: &Path) -> rusqlite::Result<Vec<ThreadIndexRow>> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut cols = HashSet::new();
        {
            let mut info = conn.prepare("PRAGMA table_info(local_thread_catalog)")?;
            for c in info.query_map([], |row| row.get::<_, String>(1))?.flatten() {
                cols.insert(c);
            }
        }
        let mut conditions = Vec::new();
        if cols.contains("host_id") {
            conditions.push("host_id = 'local'");
        }
        if cols.contains("missing_candidate") {
            conditions.push("COALESCE(missing_candidate, 0) = 0");
        }
        if cols.contains("thread_source") {
            conditions.push("COALESCE(thread_source, '') <> 'subagent'");
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let col = |name: &str| {
            if cols.contains(name) {
                name.to_string()
            } else {
                format!("NULL AS {}", name)
            }
        };
        let sql = format!(
            "SELECT thread_id, {}, {}, {}, {} FROM local_thread_catalog {}",
            col("source_updated_at"),
            col("model_provider"),
            col("display_title"),
            col("cwd"),
            where_clause
        );
        let mut stmt = conn.prepare(&sql)?;

        let rows = stmt.query_map([], |row| {
            let text = |idx: usize| -> rusqlite::Result<Option<String>> {
                Ok(text_from_value(&row.get::<_, Value>(idx)?))
            };
            Ok(ThreadIndexRow {
                id: text(0)?.unwrap_or_default(),
                rollout_path: None,
                updated_at: timestamp_from_value(&row.get::<_, Value>(1)?).unwrap_or_default(),
                model_provider: text(2)?,
                title: first_non_empty([text(3)?]).unwrap_or_else(|| "Codex 会话".to_string()),
                archived: false,
                model: None,
                cwd: text(4)?,
            })
        })?;

        Ok(rows
            .filter_map(|r| r.ok())
            .filter(|r| !r.id.is_empty())
            .collect())
    }
}

const HISTORY_ID_PREFIX: &str = "item:";

/// Rollout lines start with `{"timestamp":…,"type":…,"payload":{"type":"<kind>"`.
/// Tool output lines can be megabytes long, so lines whose payload kind cannot hold
/// dialogue text are skipped without parsing. Unknown layouts are always parsed.
fn rollout_line_may_hold_message(bytes: &[u8]) -> bool {
    const MARKER: &str = "\"payload\":{\"type\":\"";
    let head = &bytes[..bytes.len().min(256)];
    let head = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
    };
    match head.find(MARKER) {
        Some(idx) => {
            let kind = head[idx + MARKER.len()..].split('"').next().unwrap_or("");
            matches!(kind, "user_message" | "agent_message" | "message")
        }
        None => true,
    }
}

fn has_history_messages(conn: &Connection, thread_id: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM thread_items
         WHERE thread_id = ?1 AND item_type IN ('userMessage', 'agentMessage') LIMIT 1",
        [thread_id],
        |_| Ok(()),
    )
    .is_ok()
}

/// The text a user typed, without context blocks Codex adds to the prompt.
/// Attachments are listed before a `## My request` heading.
fn user_request_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || ["<in-app-browser-context", "<environment_context", "<user_instructions"]
            .iter()
            .any(|tag| trimmed.starts_with(tag))
    {
        return None;
    }
    if trimmed.starts_with("# Files mentioned by the user") {
        let mut lines = trimmed.lines();
        if lines.any(|l| l.trim_start().starts_with("## My request")) {
            let request = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            return Some(request).filter(|r| !r.is_empty());
        }
    }
    Some(trimmed.to_string())
}

struct ThreadIndexRow {
    id: String,
    rollout_path: Option<String>,
    updated_at: String,
    model_provider: Option<String>,
    title: String,
    archived: bool,
    model: Option<String>,
    cwd: Option<String>,
}

struct MergedCandidate {
    thread_id: String,
    title: String,
    rollout_path: Option<PathBuf>,
    project_path: Option<String>,
    model: Option<String>,
    providers: HashSet<String>,
    updated_at: String,
    archived: bool,
    sources: HashSet<String>,
}

struct EventRow {
    role: String,
    content: String,
    external_id: String,
    created_at: Option<String>,
}

fn select_visible_events(rows: Vec<EventRow>) -> Vec<ImportedMessagePayload> {
    let mut result = Vec::new();
    let mut parent: Option<String> = None;

    for r in rows {
        result.push(ImportedMessagePayload {
            role: r.role,
            content: r.content,
            external_id: r.external_id.clone(),
            parent_external_id: parent,
            created_at: r.created_at,
        });
        parent = Some(r.external_id);
    }
    result
}

fn extract_response_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(s);
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

struct RolloutMeta {
    title: String,
    cwd: Option<String>,
    model: Option<String>,
    model_provider: Option<String>,
    updated_at: String,
    archived: bool,
}

fn read_rollout_metadata(path: &Path, thread_id: &str) -> Option<RolloutMeta> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mtime = fs::metadata(path).ok()
        .and_then(|m| m.modified().ok())
        .map(|t| DateTime::<Utc>::from(t).to_rfc3339())
        .unwrap_or_default();

    let mut cwd = None;
    let mut model = None;
    let mut model_provider = None;
    let mut title = None;

    for (idx, line_res) in reader.lines().enumerate() {
        if idx >= 500 {
            break;
        }
        let line = match line_res {
            Ok(l) => l,
            Err(_) => continue,
        };
        let item: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let payload = match item.get("payload") {
            Some(p) if p.is_object() => p,
            _ => continue,
        };

        if item_type == "session_meta" {
            // Subagents and automatic reviews (`guardian_review`) record their parent in
            // `source.subagent`; they never appear in the Codex sidebar.
            let thread_source = payload.get("thread_source").and_then(|s| s.as_str()).unwrap_or("");
            let spawned = payload
                .get("source")
                .map(|s| s.get("subagent").is_some())
                .unwrap_or(false);
            if thread_source == "subagent" || thread_source == "guardian_review" || spawned {
                return None;
            }
            cwd = payload.get("cwd").and_then(|c| c.as_str()).map(|s| s.to_string());
            model_provider = payload.get("model_provider").and_then(|m| m.as_str()).map(|s| s.to_string());
        } else if item_type == "turn_context" {
            if let Some(m) = payload.get("model").and_then(|s| s.as_str()) {
                model = Some(m.to_string());
            }
        } else if item_type == "event_msg" && payload.get("type").and_then(|t| t.as_str()) == Some("user_message") {
            if title.is_none() {
                if let Some(msg) = payload.get("message").and_then(|m| m.as_str()) {
                    let cleaned = clean_title(msg, thread_id);
                    if !cleaned.is_empty() {
                        title = Some(cleaned);
                    }
                }
            }
        }

        if cwd.is_some() && title.is_some() && model.is_some() {
            break;
        }
    }

    Some(RolloutMeta {
        title: title.unwrap_or_else(|| "Codex 会话".to_string()),
        cwd,
        model,
        model_provider,
        updated_at: mtime,
        archived: false,
    })
}

fn clean_title(val: &str, thread_id: &str) -> String {
    let trimmed = val.trim();
    if trimmed.is_empty() || trimmed == thread_id {
        return "Codex 会话".to_string();
    }
    trimmed.chars().take(80).collect()
}

fn normalize_recency(val: &str) -> String {
    normalize_timestamp(val)
}

/// The Codex sidebar's projects and the threads it lists.
#[derive(Default)]
struct SidebarState {
    /// False when Codex keeps no project state: the file is missing or unreadable,
    /// or an older Codex build wrote it.
    available: bool,
    /// In sidebar order.
    projects: Vec<CodexProject>,
    /// Thread ID → local project ID.
    assignments: HashMap<String, String>,
    /// Threads listed among the chats outside any project.
    chats: HashSet<String>,
}

impl SidebarState {
    fn project(&self, thread_id: &str) -> Option<&CodexProject> {
        let project_id = self.assignments.get(thread_id)?;
        self.projects.iter().find(|p| p.project_id == *project_id)
    }

    /// Codex lists a thread under its assigned project, or among the chats, unless
    /// the thread is archived. A thread that is only in a project's folder is not
    /// listed under that project.
    fn placement(&self, thread_id: &str, archived: &HashSet<String>) -> CodexPlacement {
        if !archived.contains(thread_id) {
            if let Some(project) = self.project(thread_id) {
                return CodexPlacement {
                    scope: CodexScope::Project,
                    project_id: Some(project.project_id.clone()),
                };
            }
            if self.chats.contains(thread_id) {
                return CodexPlacement {
                    scope: CodexScope::Chat,
                    project_id: None,
                };
            }
        }
        CodexPlacement {
            scope: CodexScope::Hidden,
            project_id: None,
        }
    }
}

fn resolve_project_path(
    thread_id: &str,
    cwd: Option<&str>,
    sidebar: &SidebarState,
) -> Option<String> {
    if let Some(proj) = sidebar.project(thread_id) {
        return Some(proj.primary_path().to_string());
    }

    if let Some(c) = cwd {
        let norm_c = c.replace('\\', "/").to_lowercase();
        for proj in &sidebar.projects {
            for root in &proj.root_paths {
                let norm_root = root.replace('\\', "/").to_lowercase();
                if norm_c == norm_root || norm_c.starts_with(&format!("{}/", norm_root)) {
                    return Some(proj.primary_path().to_string());
                }
            }
        }
        return Some(c.to_string());
    }

    None
}
