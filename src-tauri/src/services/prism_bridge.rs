use chrono::Local;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::paths::AppPaths;
use crate::persistence::{Database, Repository};
use crate::services::DataService;

pub const BRIDGE_DIR_NAME: &str = "PrismContextBridge";
pub const PRISM_INSTANCE_FILE: &str = "prism-studio.json";
pub const VAULT_INSTANCE_FILE: &str = "context-vault.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInstanceRecord {
    pub app: String,
    pub exe_path: String,
    pub data_root: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrismStudioStatus {
    pub available: bool,
    pub running: bool,
    pub exe_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum PrismBridgeAction {
    #[serde(rename_all = "camelCase")]
    OpenWorkspace {
        path: String,
        /// "new-workspace" | "parallel-root"
        mode: String,
    },
    #[serde(rename_all = "camelCase")]
    OpenFile {
        path: String,
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        project_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    OpenCodeSnippet {
        language: String,
        code: String,
        title: String,
        #[serde(default)]
        project_path: Option<String>,
        #[serde(default)]
        auto_run: bool,
    },
    #[serde(rename_all = "camelCase")]
    OpenMarkdownNote {
        title: String,
        markdown: String,
        #[serde(default)]
        project_path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismWireEnvelope {
    token: String,
    kind: String,
    #[serde(default)]
    payload: Option<PrismBridgeAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFocusPayload {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum VaultHeadlessOrRpcRequest {
    Ping,
    #[serde(rename_all = "camelCase")]
    Focus {
        #[serde(default)]
        conversation_id: Option<String>,
        #[serde(default)]
        project_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    QuerySessions {
        #[serde(default)]
        root_paths: Vec<String>,
        #[serde(default)]
        query: String,
        #[serde(default = "default_query_limit")]
        limit: usize,
        #[serde(default)]
        include_all_projects: bool,
    },
    #[serde(rename_all = "camelCase")]
    GetSessionMarkdown {
        conversation_id: String,
    },
}

fn default_query_limit() -> usize {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultWireEnvelope {
    token: String,
    #[serde(flatten)]
    request: VaultHeadlessOrRpcRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionItem {
    pub id: String,
    pub title: String,
    pub project_path: Option<String>,
    pub source_kind: String,
    pub message_count: i64,
    pub updated_at: String,
    pub pinned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySessionsPayload {
    pub total_matched: usize,
    pub sessions: Vec<BridgeSessionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMarkdownPayload {
    pub id: String,
    pub title: String,
    pub project_path: Option<String>,
    pub source_kind: String,
    pub markdown: String,
}

#[derive(Default)]
pub struct VaultBridgeQueue {
    pending: Mutex<Vec<VaultFocusPayload>>,
}

impl VaultBridgeQueue {
    pub fn push(&self, item: VaultFocusPayload) {
        if let Ok(mut guard) = self.pending.lock() {
            guard.push(item);
        }
    }

    pub fn drain(&self) -> Vec<VaultFocusPayload> {
        if let Ok(mut guard) = self.pending.lock() {
            std::mem::take(&mut *guard)
        } else {
            Vec::new()
        }
    }
}

pub fn shared_bridge_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(env::temp_dir);
    let dir = base.join(BRIDGE_DIR_NAME);
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn read_instance_record(filename: &str) -> Option<BridgeInstanceRecord> {
    let path = shared_bridge_dir().join(filename);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn write_instance_record(filename: &str, record: &BridgeInstanceRecord) {
    let path = shared_bridge_dir().join(filename);
    if let Ok(json) = serde_json::to_string_pretty(record) {
        let _ = fs::write(path, json);
    }
}

pub fn normalize_match_path(raw: &str) -> String {
    crate::util::normalize_windows_path(raw)
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Checks whether a conversation's `project_path` belongs to (or contains) any active workspace root.
pub fn path_matches_any_root(project_path: Option<&str>, root_paths: &[String]) -> bool {
    let Some(proj_raw) = project_path else {
        return false;
    };
    let proj = normalize_match_path(proj_raw);
    if proj.is_empty() {
        return false;
    }
    for root_raw in root_paths {
        let root = normalize_match_path(root_raw);
        if root.is_empty() {
            continue;
        }
        if proj == root
            || proj.starts_with(&format!("{}\\", root))
            || root.starts_with(&format!("{}\\", proj))
        {
            return true;
        }
    }
    false
}

pub fn execute_vault_data_query(
    db: &Database,
    request: &VaultHeadlessOrRpcRequest,
) -> BridgeResponse {
    match request {
        VaultHeadlessOrRpcRequest::Ping => BridgeResponse {
            ok: true,
            error: None,
            data: None,
        },
        VaultHeadlessOrRpcRequest::Focus { .. } => BridgeResponse {
            ok: false,
            error: Some("无界面查询模式不支持窗口聚焦操作".to_string()),
            data: None,
        },
        VaultHeadlessOrRpcRequest::QuerySessions {
            root_paths,
            query,
            limit,
            include_all_projects,
        } => {
            let repo = Repository::new(db);
            let fetch_cap = (*limit).clamp(1, 500);
            let raw_list = match repo.search_conversations_with_archived(query, 500, false) {
                Ok(list) => list,
                Err(e) => {
                    return BridgeResponse {
                        ok: false,
                        error: Some(format!("查询会话失败：{}", e)),
                        data: None,
                    };
                }
            };

            let filter_by_roots = !*include_all_projects && !root_paths.is_empty();
            let matched: Vec<BridgeSessionItem> = raw_list
                .into_iter()
                .filter(|c| {
                    if !filter_by_roots {
                        true
                    } else {
                        path_matches_any_root(c.project_path.as_deref(), root_paths)
                    }
                })
                .map(|c| BridgeSessionItem {
                    id: c.id,
                    title: c.title,
                    project_path: c.project_path,
                    source_kind: c.source_kind,
                    message_count: c.message_count,
                    updated_at: c.updated_at.to_rfc3339(),
                    pinned_at: c.pinned_at,
                })
                .collect();

            let total_matched = matched.len();
            let sessions: Vec<BridgeSessionItem> =
                matched.into_iter().take(fetch_cap).collect();

            BridgeResponse {
                ok: true,
                error: None,
                data: serde_json::to_value(QuerySessionsPayload {
                    total_matched,
                    sessions,
                })
                .ok(),
            }
        }
        VaultHeadlessOrRpcRequest::GetSessionMarkdown { conversation_id } => {
            let service = DataService::new(db);
            match service.render_conversation_markdown(conversation_id) {
                Ok((conv, markdown)) => BridgeResponse {
                    ok: true,
                    error: None,
                    data: serde_json::to_value(SessionMarkdownPayload {
                        id: conv.id,
                        title: conv.title,
                        project_path: conv.project_path,
                        source_kind: conv.source_kind,
                        markdown,
                    })
                    .ok(),
                },
                Err(e) => BridgeResponse {
                    ok: false,
                    error: Some(e),
                    data: None,
                },
            }
        }
    }
}

fn send_json_line<T: Serialize>(
    port: u16,
    payload: &T,
    timeout_ms: u64,
) -> Result<BridgeResponse, String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = Duration::from_millis(timeout_ms);
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("连接桥接端口失败: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms.max(1500))));
    let _ = stream.set_write_timeout(Some(timeout));

    let mut line = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("发送桥接指令失败: {e}"))?;
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .map_err(|e| format!("读取桥接响应失败: {e}"))?;
    serde_json::from_str::<BridgeResponse>(resp_line.trim())
        .map_err(|e| format!("解析桥接响应失败: {e}"))
}

/// Checks if an existing Context Vault instance is running; if `focus_payload` is Some,
/// forwards focus to it so we never spawn duplicate Context Vault windows.
pub fn try_forward_to_running_vault(focus: Option<VaultFocusPayload>) -> bool {
    let Some(record) = read_instance_record(VAULT_INSTANCE_FILE) else {
        return false;
    };
    if record.pid == std::process::id() {
        return false;
    }
    let req = match focus {
        Some(f) => VaultHeadlessOrRpcRequest::Focus {
            conversation_id: f.conversation_id,
            project_path: f.project_path,
        },
        None => VaultHeadlessOrRpcRequest::Ping,
    };
    let env = VaultWireEnvelope {
        token: record.token,
        request: req,
    };
    matches!(send_json_line(record.port, &env, 250), Ok(resp) if resp.ok)
}

pub fn load_handoff_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    let _ = fs::remove_file(path);
    serde_json::from_str(&raw).ok()
}

/// Resolves either a Context Vault `conversations.id` OR a client `source_id`
/// (e.g. `conversation://39eb79da-...` from Antigravity/Codex/Claude) to `(vault_id, source_kind)`.
pub fn resolve_vault_conversation(db: &Database, id_or_source_id: &str) -> Option<(String, String)> {
    let trimmed = id_or_source_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let repo = Repository::new(db);
    if let Ok(Some(conv)) = repo.get_conversation(trimmed) {
        return Some((conv.id, conv.source_kind));
    }
    let conn = db.lock_conn();
    for source_type in ["antigravity_transcript", "codex_rollout", "claude_code"] {
        let lower = format!("{}:{}:", source_type, trimmed);
        let upper = format!("{}:{};", source_type, trimmed);
        if let Ok(found) = conn.query_row(
            "SELECT c.id, c.source_kind
             FROM messages m
             JOIN conversations c ON c.id = m.conversation_id
             WHERE m.source_type = ?1 AND m.source_external_id >= ?2 AND m.source_external_id < ?3
             LIMIT 1",
            rusqlite::params![source_type, lower, upper],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ) {
            return Some(found);
        }
    }
    None
}

pub fn focus_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn start_vault_bridge_server(
    app: AppHandle,
    paths: &AppPaths,
    database: Arc<Database>,
    queue: Arc<VaultBridgeQueue>,
) {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(_) => return,
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(_) => return,
    };
    let token = Uuid::new_v4().to_string();
    let exe_path = env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    write_instance_record(
        VAULT_INSTANCE_FILE,
        &BridgeInstanceRecord {
            app: "context-vault".to_string(),
            exe_path,
            data_root: paths.root.to_string_lossy().to_string(),
            port,
            token: token.clone(),
            pid: std::process::id(),
            updated_at: Local::now().to_rfc3339(),
        },
    );

    std::thread::spawn(move || {
        for incoming in listener.incoming().flatten() {
            let app_clone = app.clone();
            let db_clone = database.clone();
            let queue_clone = queue.clone();
            let expected_token = token.clone();
            std::thread::spawn(move || {
                let _ = incoming.set_read_timeout(Some(Duration::from_millis(2000)));
                let _ = incoming.set_write_timeout(Some(Duration::from_millis(2000)));
                let mut reader = BufReader::new(&incoming);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                    return;
                }
                let resp = match serde_json::from_str::<VaultWireEnvelope>(line.trim()) {
                    Ok(env) if env.token == expected_token => match env.request {
                        VaultHeadlessOrRpcRequest::Focus {
                            conversation_id,
                            project_path,
                        } => {
                            let resolved = conversation_id
                                .as_deref()
                                .and_then(|cid| resolve_vault_conversation(&db_clone, cid));
                            let payload = VaultFocusPayload {
                                conversation_id: resolved
                                    .as_ref()
                                    .map(|(id, _)| id.clone())
                                    .or(conversation_id),
                                source_kind: resolved.map(|(_, sk)| sk),
                                project_path,
                            };
                            queue_clone.push(payload.clone());
                            focus_main_window(&app_clone);
                            let _ = app_clone.emit("cv://bridge-focus", payload);
                            BridgeResponse {
                                ok: true,
                                error: None,
                                data: None,
                            }
                        }
                        other => execute_vault_data_query(&db_clone, &other),
                    },
                    Ok(_) => BridgeResponse {
                        ok: false,
                        error: Some("桥接令牌不匹配".to_string()),
                        data: None,
                    },
                    Err(e) => BridgeResponse {
                        ok: false,
                        error: Some(format!("无效请求: {e}")),
                        data: None,
                    },
                };

                if let Ok(mut json) = serde_json::to_string(&resp) {
                    json.push('\n');
                    let mut writer = incoming;
                    let _ = writer.write_all(json.as_bytes());
                    let _ = writer.flush();
                }
            });
        }
    });
}

/// Discovers `PrismStudio.exe` automatically without hardcoding user-specific paths.
pub fn discover_prism_studio_exe() -> Option<PathBuf> {
    if let Ok(env_exe) = env::var("PRISM_STUDIO_EXE") {
        let p = PathBuf::from(env_exe);
        if p.is_file() {
            return Some(p);
        }
    }

    if let Some(record) = read_instance_record(PRISM_INSTANCE_FILE) {
        let p = PathBuf::from(&record.exe_path);
        if p.is_file() {
            return Some(p);
        }
    }

    let mut search_roots = Vec::new();
    if let Ok(cur_exe) = env::current_exe() {
        for ancestor in cur_exe.ancestors().skip(1).take(5) {
            search_roots.push(ancestor.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors().take(4) {
            search_roots.push(ancestor.to_path_buf());
        }
    }

    let relative_candidates = [
        r"md-pdf-viewer\releases\desktop\PrismStudio.exe",
        r"md-pdf-viewer\releases\portable\PrismStudio.exe",
        r"PrismStudio\PrismStudio.exe",
        r"PrismStudio.exe",
    ];

    for root in search_roots {
        for rel in relative_candidates {
            let candidate = root.join(rel);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn is_prism_studio_running() -> bool {
    let Some(record) = read_instance_record(PRISM_INSTANCE_FILE) else {
        return false;
    };
    let env = PrismWireEnvelope {
        token: record.token,
        kind: "ping".to_string(),
        payload: None,
    };
    matches!(send_json_line(record.port, &env, 220), Ok(resp) if resp.ok)
}

pub fn get_prism_status() -> PrismStudioStatus {
    let running = is_prism_studio_running();
    let exe = discover_prism_studio_exe();
    PrismStudioStatus {
        available: running || exe.is_some(),
        running,
        exe_path: exe.map(|p| p.to_string_lossy().to_string()),
    }
}

/// Sends an action to an already-running `PrismStudio` window in < 5ms, or cold-launches
/// `PrismStudio.exe` with a handoff file if it isn't running yet.
pub fn dispatch_to_prism_studio(action: PrismBridgeAction) -> Result<String, String> {
    if let Some(record) = read_instance_record(PRISM_INSTANCE_FILE) {
        let env = PrismWireEnvelope {
            token: record.token,
            kind: "dispatch".to_string(),
            payload: Some(action.clone()),
        };
        if let Ok(resp) = send_json_line(record.port, &env, 350) {
            if resp.ok {
                return Ok("reused-window".to_string());
            }
        }
    }

    let exe_path = discover_prism_studio_exe().ok_or_else(|| {
        "未找到棱镜工坊 (PrismStudio.exe)。请先启动一次 PrismStudio，或构建 md-pdf-viewer 发布包。"
            .to_string()
    })?;

    let handoff_path =
        shared_bridge_dir().join(format!("prism-handoff-{}.json", Uuid::new_v4()));
    let json = serde_json::to_string_pretty(&action)
        .map_err(|e| format!("序列化棱镜工坊参数失败: {e}"))?;
    fs::write(&handoff_path, json).map_err(|e| format!("写入桥接交接文件失败: {e}"))?;

    let mut cmd = Command::new(&exe_path);
    cmd.arg("--bridge-handoff").arg(&handoff_path);
    if let Some(parent) = exe_path.parent() {
        cmd.current_dir(parent);
    }
    cmd.spawn()
        .map_err(|e| format!("启动棱镜工坊失败 ({}): {}", exe_path.display(), e))?;

    Ok("launched-new".to_string())
}

/// Resolves the best database root for headless queries: prefers the `dataRoot` of the
/// most recently run `Context-Vault` instance if its `conversations.db` exists, otherwise
/// falls back to `AppPaths::discover(None)`.
pub fn discover_best_vault_paths() -> Result<AppPaths, std::io::Error> {
    if let Some(record) = read_instance_record(VAULT_INSTANCE_FILE) {
        let root = PathBuf::from(&record.data_root);
        if root.join("data").join("conversations.db").is_file() {
            if let Ok(paths) = AppPaths::discover(Some(&root)) {
                return Ok(paths);
            }
        }
    }
    let default_paths = AppPaths::discover(None)?;
    if default_paths.database.is_file() {
        if let Ok(meta) = fs::metadata(&default_paths.database) {
            if meta.len() > 32 * 1024 {
                return Ok(default_paths);
            }
        }
    }
    if let Some(sys_local) = dirs::data_local_dir() {
        let sys_root = sys_local.join("ContextRestoration");
        let sys_db = sys_root.join("data").join("conversations.db");
        if sys_db.is_file() {
            if let Ok(paths) = AppPaths::discover(Some(&sys_root)) {
                return Ok(paths);
            }
        }
    }
    Ok(default_paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches_any_root_bidirectional() {
        let roots = vec![r"D:\huangjunjie\fine-tune the model using field data-total".to_string()];

        // Exact match (case & slash insensitive)
        assert!(path_matches_any_root(
            Some("d:/huangjunjie/fine-tune the model using field data-total/"),
            &roots
        ));

        // Conversation inside subfolder of workspace root
        assert!(path_matches_any_root(
            Some(r"D:\huangjunjie\fine-tune the model using field data-total\01_copper-temperature-lab"),
            &roots
        ));

        // Workspace opened on a subfolder of the conversation's repo root
        let sub_roots =
            vec![r"D:\huangjunjie\fine-tune the model using field data-total\gemini".to_string()];
        assert!(path_matches_any_root(
            Some(r"D:\huangjunjie\fine-tune the model using field data-total"),
            &sub_roots
        ));

        // Sibling prefix collision must NOT match (`proj` vs `proj_other`)
        assert!(!path_matches_any_root(
            Some(r"D:\huangjunjie\fine-tune the model using field data-total-backup"),
            &roots
        ));

        assert!(!path_matches_any_root(None, &roots));
    }

    #[test]
    fn test_execute_vault_data_query_filters_and_renders() {
        let temp_dir = env::temp_dir().join(format!("cv_bridge_test_{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);
        let db = Database::new(&temp_dir.join("test.db"), &temp_dir.join("backups")).unwrap();

        {
            let conn = db.lock_conn();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO conversations (id, title, source_title, title_user_edited, system_prompt, project_path, source_kind, archived, created_at, updated_at)
                 VALUES ('c-match', '算子实验讨论', '算子实验讨论', 0, '', 'D:\\proj\\alpha', 'antigravity', 0, ?1, ?2)",
                rusqlite::params![now, now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO messages (id, conversation_id, sequence_no, role, content, status, created_at, updated_at)
                 VALUES ('m1', 'c-match', 1, 'user', '如何优化 FNO 收敛速度？', 'complete', ?1, ?2)",
                rusqlite::params![now, now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversations (id, title, source_title, title_user_edited, system_prompt, project_path, source_kind, archived, created_at, updated_at)
                 VALUES ('c-other', '另一个无关项目', '另一个无关项目', 0, '', 'D:\\proj\\beta', 'codex_main', 0, ?1, ?2)",
                rusqlite::params![now, now],
            )
            .unwrap();
        }

        let resp = execute_vault_data_query(
            &db,
            &VaultHeadlessOrRpcRequest::QuerySessions {
                root_paths: vec![r"D:\proj\alpha".to_string()],
                query: String::new(),
                limit: 20,
                include_all_projects: false,
            },
        );
        assert!(resp.ok);
        let payload: QuerySessionsPayload = serde_json::from_value(resp.data.unwrap()).unwrap();
        assert_eq!(payload.total_matched, 1);
        assert_eq!(payload.sessions[0].id, "c-match");

        let md_resp = execute_vault_data_query(
            &db,
            &VaultHeadlessOrRpcRequest::GetSessionMarkdown {
                conversation_id: "c-match".to_string(),
            },
        );
        assert!(md_resp.ok);
        let md_payload: SessionMarkdownPayload =
            serde_json::from_value(md_resp.data.unwrap()).unwrap();
        assert!(md_payload.markdown.contains("# 算子实验讨论"));
        assert!(md_payload.markdown.contains("如何优化 FNO 收敛速度？"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
