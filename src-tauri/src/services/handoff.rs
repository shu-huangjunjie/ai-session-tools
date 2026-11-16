//! 跨工具会话接力（Handoff）：支持将任意来源会话一键转入 Codex / Claude / Antigravity 或切换账号后继续对话。
//!
//! 同时提供两种模式：
//! - **模式 A（`native`）**：以全新会话 UUID 无损注入目标客户端本地存储，并通过 CLI `--resume` / `--conversation` 或桌面端直达继续。
//! - **模式 B（`compact`）**：在项目内生成精简上下文交接文档 `.context-vault/handoff-<id>.md`（含 Git 变更摘要与最近 N 轮对话），以首句引导提示词拉起新工具。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Local, SecondsFormat, Utc};
use rusqlite::{types::Value as SqlValue, Connection, ToSql};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{CanonicalMessage, MessageRole};
use crate::paths::AppPaths;
use crate::persistence::{backup_sqlite, Database, Repository};
use crate::services::{
    quick_launch, AccountClient, AntigravityAccountManager, AntigravityClientController,
    AntigravityRecoveryService, ClaudeAccountManager, ClaudeClientController, ClaudePublisher,
    ClaudeRecoveryService, CodexAccountManager, CodexClientController, CodexRecoveryService,
    QuickLaunchService,
};
use crate::util::{normalize_windows_path, percent_encode_component, percent_encode_path};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffMode {
    Native,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRequest {
    pub conversation_id: String,
    pub target_client: AccountClient,
    pub target_account_id: Option<String>,
    pub mode: HandoffMode,
    pub tool_id: String,
    pub folder_path: Option<String>,
    pub terminal: Option<String>,
    pub extra_cli_args: Option<String>,
    pub recent_message_limit: Option<usize>,
    pub follow_up_prompt: Option<String>,
    pub sync_latest_first: bool,
    pub prepare_only: bool,
    pub restart_desktop_if_running: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffResult {
    pub mode: HandoffMode,
    pub target_client: AccountClient,
    pub target_session_id: Option<String>,
    pub handoff_file_path: Option<String>,
    pub working_dir: Option<String>,
    pub workspace_created: bool,
    pub workspace_project_id: Option<String>,
    pub deep_link_uri: Option<String>,
    pub session_title: String,
    pub cli_command: String,
    pub clipboard_prompt: String,
    pub synced_new_messages: usize,
    pub total_messages: usize,
    pub launched: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct HandoffTargetHomes {
    codex_home: PathBuf,
    claude_home: PathBuf,
    gemini_home: PathBuf,
    claude_data_dir: Option<PathBuf>,
    antigravity_app_storage: Option<PathBuf>,
}

impl HandoffTargetHomes {
    fn from_paths(paths: &AppPaths) -> Self {
        Self {
            codex_home: paths.codex_home(),
            claude_home: paths.claude_home(),
            gemini_home: paths.gemini_home(),
            claude_data_dir: dirs::data_local_dir().map(|d| d.join("Claude-Data")),
            antigravity_app_storage: dirs::config_dir()
                .map(|d| d.join("Antigravity").join("app_storage.json")),
        }
    }
}

pub fn execute_handoff(
    paths: &AppPaths,
    db: &Database,
    req: HandoffRequest,
) -> Result<HandoffResult, String> {
    let homes = HandoffTargetHomes::from_paths(paths);
    execute_handoff_with_homes(paths, db, req, &homes)
}

fn execute_handoff_with_homes(
    paths: &AppPaths,
    db: &Database,
    req: HandoffRequest,
    homes: &HandoffTargetHomes,
) -> Result<HandoffResult, String> {
    let repo = Repository::new(db);
    let conv = repo
        .get_conversation(&req.conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未找到对应的本地会话，请先扫描或刷新列表".to_string())?;

    let (working_dir, workspace_created) = resolve_or_create_working_dir(
        req.folder_path.as_deref(),
        conv.project_path.as_deref(),
        &conv.title,
    )?;
    let ql = QuickLaunchService::new(&paths.data);

    if !req.prepare_only {
        ql.validate_launch_request(&req.tool_id, Some(&working_dir))?;
    }

    let mut warnings = Vec::new();
    let mut synced_new_messages = 0;

    if req.sync_latest_first {
        synced_new_messages =
            sync_single_conversation_if_available(paths, db, &conv.id, &conv.source_kind, &mut warnings);
    }

    let all_msgs = repo
        .load_active_path_or_chronological(&req.conversation_id)
        .map_err(|e| e.to_string())?;
    let messages: Vec<CanonicalMessage> = all_msgs
        .into_iter()
        .filter(|m| {
            matches!(m.role, MessageRole::User | MessageRole::Assistant)
                && !m.content.trim().is_empty()
        })
        .collect();

    if messages.is_empty() {
        return Err("当前会话没有可交接的用户或助手文本消息".to_string());
    }

    let total_messages = messages.len();

    // 若选择了目标账号，按安全策略切换账号凭据
    if let Some(acc_id) = req
        .target_account_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        switch_account_for_handoff(
            paths,
            req.target_client,
            acc_id,
            &req.tool_id,
            req.restart_desktop_if_running.unwrap_or(false),
        )?;
    } else if !req.prepare_only && req.restart_desktop_if_running.unwrap_or(false) {
        stop_desktop_if_requested(req.target_client, &req.tool_id)?;
    }

    // 无论模式 A 还是模式 B，均确保目标工具侧边栏已存在对应工作文件夹/项目（有则复用、无则新建）
    let mut workspace_project_id: Option<String> = None;
    let mut codex_ids = CodexProjectIds::default();
    match req.target_client {
        AccountClient::Codex => {
            codex_ids = ensure_codex_workspace_project(
                &homes.codex_home,
                &paths.backups,
                &working_dir,
                &mut warnings,
            );
            workspace_project_id = codex_ids
                .legacy_project_id
                .clone()
                .or_else(|| codex_ids.state_project_id.clone());
        }
        AccountClient::Claude => {
            match ensure_claude_workspace_preset_in_dir(
                &homes.claude_home,
                homes.claude_data_dir.as_deref(),
                &working_dir,
            ) {
                Ok(pid) => workspace_project_id = pid,
                Err(e) => warnings.push(format!("同步 Claude 工作区预设失败：{}", e)),
            }
        }
        AccountClient::Antigravity => {
            match ensure_antigravity_workspace_project(
                &homes.gemini_home,
                homes.antigravity_app_storage.as_deref(),
                &working_dir,
            ) {
                Ok(pid) => workspace_project_id = Some(pid),
                Err(e) => warnings.push(format!("创建/匹配 Antigravity 工作区失败：{}", e)),
            }
        }
    }

    let handoff_title = format_handoff_title(&conv.title);
    let clean_flags = strip_leading_cli_bin(req.extra_cli_args.as_deref());
    let is_editor = is_editor_tool(&req.tool_id);
    let short_id: String = Uuid::new_v4().simple().to_string().chars().take(8).collect();

    struct PreparedLaunch {
        target_session_id: Option<String>,
        handoff_file_path: Option<String>,
        cli_invoke_args: Option<String>,
        cli_command: String,
        clipboard_prompt: String,
        deep_link_uri: Option<String>,
    }

    let prepared = match (req.mode, req.target_client) {
        (HandoffMode::Native, AccountClient::Codex) => {
            let session_id = Uuid::new_v4().to_string();
            inject_codex_session_with_projects(
                &homes.codex_home,
                &paths.backups,
                &session_id,
                &handoff_title,
                Some(&working_dir),
                &codex_ids,
                &messages,
                &mut warnings,
            )?;

            let md_file = if is_editor {
                let (fp, _) = create_handoff_markdown_file(
                    paths,
                    Some(&working_dir),
                    &short_id,
                    &handoff_title,
                    &conv.source_kind,
                    &messages,
                    0,
                    req.follow_up_prompt.as_deref(),
                )?;
                Some(fp.to_string_lossy().to_string())
            } else {
                None
            };

            let (bin, invoke_args) =
                build_native_cli_args(AccountClient::Codex, &session_id, clean_flags.as_deref());
            let cli_cmd = format!("{} {}", bin, invoke_args);
            let deep_link_uri = if req.tool_id == "codex_app" {
                Some(format!("codex://threads/{}", session_id))
            } else {
                None
            };
            let clipboard_prompt = if is_editor {
                md_file
                    .as_ref()
                    .map(|_| build_compact_boot_prompt(&format!(".context-vault/handoff-{}.md", short_id)))
                    .unwrap_or_else(|| cli_cmd.clone())
            } else {
                cli_cmd.clone()
            };

            PreparedLaunch {
                target_session_id: Some(session_id),
                handoff_file_path: md_file,
                cli_invoke_args: Some(invoke_args),
                cli_command: cli_cmd,
                clipboard_prompt,
                deep_link_uri,
            }
        }
        (HandoffMode::Native, AccountClient::Claude) => {
            let session_id = Uuid::new_v4().to_string();
            inject_claude_session(
                &homes.claude_home,
                &paths.backups,
                &session_id,
                &handoff_title,
                Some(&working_dir),
                &messages,
            )?;

            let md_file = if is_editor {
                let (fp, _) = create_handoff_markdown_file(
                    paths,
                    Some(&working_dir),
                    &short_id,
                    &handoff_title,
                    &conv.source_kind,
                    &messages,
                    0,
                    req.follow_up_prompt.as_deref(),
                )?;
                Some(fp.to_string_lossy().to_string())
            } else {
                None
            };

            let (bin, invoke_args) =
                build_native_cli_args(AccountClient::Claude, &session_id, clean_flags.as_deref());
            let cli_cmd = format!("{} {}", bin, invoke_args);
            let deep_link_uri = if req.tool_id == "claude_app" {
                Some(format!("claude://resume?session={}", session_id))
            } else {
                None
            };
            let clipboard_prompt = if is_editor {
                build_compact_boot_prompt(&format!(".context-vault/handoff-{}.md", short_id))
            } else {
                cli_cmd.clone()
            };

            PreparedLaunch {
                target_session_id: Some(session_id),
                handoff_file_path: md_file,
                cli_invoke_args: Some(invoke_args),
                cli_command: cli_cmd,
                clipboard_prompt,
                deep_link_uri,
            }
        }
        (HandoffMode::Native, AccountClient::Antigravity) => {
            // Antigravity 2.0 通过 conversations/<id>.db + 内部 protobuf 维护实时轨迹；
            // 因此以“全量原文（不截断）交接文档 + agy --project <id> -i 启动真实在线会话”实现无损续聊。
            let (file_path, prompt_ref) = create_handoff_markdown_file(
                paths,
                Some(&working_dir),
                &short_id,
                &handoff_title,
                &conv.source_kind,
                &messages,
                0,
                req.follow_up_prompt.as_deref(),
            )?;
            let boot_prompt = build_compact_boot_prompt(&prompt_ref);
            let (bin, invoke_args) = build_antigravity_cli_args(
                &boot_prompt,
                workspace_project_id.as_deref(),
                clean_flags.as_deref(),
            );
            let cli_cmd = format!("{} {}", bin, invoke_args);

            PreparedLaunch {
                target_session_id: None,
                handoff_file_path: Some(file_path.to_string_lossy().to_string()),
                cli_invoke_args: Some(invoke_args),
                cli_command: cli_cmd,
                clipboard_prompt: boot_prompt,
                deep_link_uri: None,
            }
        }
        (HandoffMode::Compact, client) => {
            let (file_path, prompt_ref) = create_handoff_markdown_file(
                paths,
                Some(&working_dir),
                &short_id,
                &handoff_title,
                &conv.source_kind,
                &messages,
                req.recent_message_limit.unwrap_or(10),
                req.follow_up_prompt.as_deref(),
            )?;
            let boot_prompt = build_compact_boot_prompt(&prompt_ref);
            let (bin, invoke_args) = if client == AccountClient::Antigravity {
                build_antigravity_cli_args(
                    &boot_prompt,
                    workspace_project_id.as_deref(),
                    clean_flags.as_deref(),
                )
            } else {
                build_compact_cli_args(client, &boot_prompt, clean_flags.as_deref())
            };
            let cli_cmd = format!("{} {}", bin, invoke_args);
            let deep_link_uri = match (client, req.tool_id.as_str()) {
                (AccountClient::Codex, "codex_app") => Some(format!(
                    "codex://threads/new?path={}&prompt={}",
                    percent_encode_component(&working_dir),
                    percent_encode_component(&boot_prompt)
                )),
                (AccountClient::Claude, "claude_app") => Some(format!(
                    "claude://code/new?folder={}&prompt={}",
                    percent_encode_component(&working_dir),
                    percent_encode_component(&boot_prompt)
                )),
                _ => None,
            };

            PreparedLaunch {
                target_session_id: None,
                handoff_file_path: Some(file_path.to_string_lossy().to_string()),
                cli_invoke_args: Some(invoke_args),
                cli_command: cli_cmd,
                clipboard_prompt: boot_prompt,
                deep_link_uri,
            }
        }
    };

    let launched = if req.prepare_only {
        false
    } else if let Some(uri) = prepared.deep_link_uri.as_deref() {
        let label = if req.target_client == AccountClient::Codex {
            "Codex"
        } else {
            "Claude"
        };
        quick_launch::open_shell_uri(uri, label)?;
        true
    } else {
        let launch_args = if is_cli_tool_for_client(req.target_client, &req.tool_id) {
            prepared.cli_invoke_args.as_deref()
        } else {
            None
        };
        let open_file = if is_editor {
            prepared.handoff_file_path.as_deref().map(PathBuf::from)
        } else {
            None
        };
        ql.launch_tool_with_options_and_file(
            &req.tool_id,
            Some(&working_dir),
            req.terminal.as_deref(),
            launch_args,
            open_file.as_deref(),
        )?;
        true
    };

    Ok(HandoffResult {
        mode: req.mode,
        target_client: req.target_client,
        target_session_id: prepared.target_session_id,
        handoff_file_path: prepared.handoff_file_path,
        working_dir: Some(working_dir),
        workspace_created,
        workspace_project_id,
        deep_link_uri: prepared.deep_link_uri,
        session_title: handoff_title,
        cli_command: prepared.cli_command,
        clipboard_prompt: prepared.clipboard_prompt,
        synced_new_messages,
        total_messages,
        launched,
        warnings,
    })
}

fn clean_working_dir_path(raw: &str) -> String {
    let base = normalize_windows_path(raw);
    #[cfg(windows)]
    {
        let mut s = base.trim().replace('/', "\\");
        while s.len() > 3 && s.ends_with('\\') {
            s.pop();
        }
        let bytes = s.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_lowercase() {
            let upper = (bytes[0] as char).to_ascii_uppercase();
            s = format!("{}{}", upper, &s[1..]);
        }
        s
    }
    #[cfg(not(windows))]
    {
        let mut s = base.trim().to_string();
        while s.len() > 1 && s.ends_with('/') {
            s.pop();
        }
        s
    }
}

fn safe_workspace_slug(title: &str) -> String {
    let cleaned = crate::util::sanitize_file_name(title, "handoff-workspace");
    let short: String = cleaned.chars().take(48).collect();
    short.trim().to_string()
}

fn folder_display_name(working_dir: &str) -> String {
    Path::new(working_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Workspace")
        .to_string()
}

fn format_handoff_title(raw_title: &str) -> String {
    let collapsed: String = raw_title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut stripped = collapsed.as_str();
    loop {
        let next = stripped
            .strip_prefix("[续聊]")
            .or_else(|| stripped.strip_prefix("[Handoff]"))
            .map(str::trim_start);
        match next {
            Some(rest) => stripped = rest,
            None => break,
        }
    }
    let clean = if stripped.is_empty() {
        "新对话"
    } else {
        stripped
    };
    let count = clean.chars().count();
    let short: String = if count > 80 {
        format!("{}...", clean.chars().take(80).collect::<String>())
    } else {
        clean.to_string()
    };
    format!("[续聊] {}", short)
}

pub fn resolve_or_create_working_dir(
    requested: Option<&str>,
    fallback_project: Option<&str>,
    conv_title: &str,
) -> Result<(String, bool), String> {
    let chosen = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            fallback_project
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let root = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("ContextVault-Workspaces")
                .join(safe_workspace_slug(conv_title));
            root.to_string_lossy().to_string()
        });

    let clean_dir = clean_working_dir_path(&chosen);
    let path = Path::new(&clean_dir);
    if path.is_dir() {
        return Ok((clean_dir, false));
    }
    if path.exists() {
        return Err(format!("指定工作路径已存在且不是文件夹：{}", clean_dir));
    }
    fs::create_dir_all(path)
        .map_err(|e| format!("创建工作目录失败 {}: {}", clean_dir, e))?;
    Ok((clean_dir, true))
}

fn sync_single_conversation_if_available(
    paths: &AppPaths,
    db: &Database,
    conv_id: &str,
    source_kind: &str,
    warnings: &mut Vec<String>,
) -> usize {
    let repo = Repository::new(db);
    let conv_id_owned = conv_id.to_string();
    let ids = std::slice::from_ref(&conv_id_owned);
    match source_kind {
        "codex_main" => {
            let Ok(map) = repo.source_ids_for_conversations(CodexRecoveryService::SOURCE_TYPE, ids)
            else {
                return 0;
            };
            let Some(tid) = map.get(conv_id) else {
                return 0;
            };
            let svc = CodexRecoveryService::new(db, Some(&paths.codex_home()));
            let report = svc.scan();
            if let Some(cand) = report.candidates.into_iter().find(|c| &c.thread_id == tid) {
                let res = svc.recover(&[cand]);
                warnings.extend(res.warnings);
                return res.message_count;
            }
            0
        }
        "claude" => {
            let Ok(map) = repo.source_ids_for_conversations(ClaudeRecoveryService::SOURCE_TYPE, ids)
            else {
                return 0;
            };
            let Some(sid) = map.get(conv_id) else {
                return 0;
            };
            let svc = ClaudeRecoveryService::new(db, Some(&paths.claude_home()));
            let report = svc.scan();
            if let Some(cand) = report.candidates.into_iter().find(|c| &c.session_id == sid) {
                let res = svc.recover(&[cand]);
                warnings.extend(res.warnings);
                return res.message_count;
            }
            0
        }
        "antigravity" => {
            let Ok(map) =
                repo.source_ids_for_conversations(AntigravityRecoveryService::SOURCE_TYPE, ids)
            else {
                return 0;
            };
            let Some(ag_id) = map.get(conv_id) else {
                return 0;
            };
            let svc = AntigravityRecoveryService::new(db, Some(&paths.antigravity_home()));
            let report = svc.scan();
            if let Some(cand) = report
                .candidates
                .into_iter()
                .find(|c| &c.conversation_id == ag_id)
            {
                let res = svc.recover(&[cand]);
                warnings.extend(res.warnings);
                return res.message_count;
            }
            0
        }
        _ => 0,
    }
}

fn switch_account_for_handoff(
    paths: &AppPaths,
    target_client: AccountClient,
    acc_id: &str,
    tool_id: &str,
    restart_desktop: bool,
) -> Result<(), String> {
    match target_client {
        AccountClient::Codex => {
            let manager = CodexAccountManager::new(&paths.codex_home(), &paths.data, &paths.backups);
            let cur = manager.state()?;
            let already = cur
                .accounts
                .iter()
                .any(|a| a.id == acc_id && a.is_current);
            if !already {
                let controller = CodexClientController::new();
                if (tool_id == "codex_app" || restart_desktop) && controller.is_running() {
                    controller.stop(15.0)?;
                }
                manager.switch(acc_id)?;
            } else if tool_id == "codex_app" && restart_desktop {
                let controller = CodexClientController::new();
                if controller.is_running() {
                    controller.stop(15.0)?;
                }
            }
        }
        AccountClient::Claude => {
            let manager = ClaudeAccountManager::new(
                &paths.claude_home(),
                &paths.claude_json(),
                &paths.data,
                &paths.backups,
            );
            let cur = manager.state()?;
            let already = cur
                .accounts
                .iter()
                .any(|a| a.id == acc_id && a.is_current);
            if !already {
                let controller = ClaudeClientController::new();
                if tool_id == "claude_app" && controller.is_running() {
                    controller.stop(15.0)?;
                }
                manager.switch(acc_id)?;
            } else if tool_id == "claude_app" && restart_desktop {
                let controller = ClaudeClientController::new();
                if controller.is_running() {
                    controller.stop(15.0)?;
                }
            }
        }
        AccountClient::Antigravity => {
            let manager = AntigravityAccountManager::new_live(
                &crate::services::antigravity_accounts::default_state_db(),
                &paths.data,
                &paths.backups,
                Some(paths.gemini_home()),
            );
            let cur = manager.state()?;
            let already = cur
                .accounts
                .iter()
                .any(|a| a.id == acc_id && a.is_current);
            if !already {
                let has_legacy_db =
                    crate::services::antigravity_accounts::default_state_db().is_file();
                let controller = AntigravityClientController::new();
                let running = controller.is_running();
                let want_stop = tool_id == "antigravity_app" || restart_desktop;
                if want_stop && running {
                    controller.stop(15.0)?;
                } else if !want_stop && has_legacy_db && running {
                    return Err(
                        "Antigravity 正在运行，切换旧版账号前请先关闭它，或勾选“启动前关闭已运行的桌面客户端”"
                            .to_string(),
                    );
                }
                manager.switch(acc_id)?;
            } else if tool_id == "antigravity_app" && restart_desktop {
                let controller = AntigravityClientController::new();
                if controller.is_running() {
                    controller.stop(15.0)?;
                }
            }
        }
    }
    Ok(())
}

fn stop_desktop_if_requested(target_client: AccountClient, tool_id: &str) -> Result<(), String> {
    match target_client {
        AccountClient::Codex if tool_id == "codex_app" => {
            let c = CodexClientController::new();
            if c.is_running() {
                c.stop(15.0)?;
            }
        }
        AccountClient::Claude if tool_id == "claude_app" => {
            let c = ClaudeClientController::new();
            if c.is_running() {
                c.stop(15.0)?;
            }
        }
        AccountClient::Antigravity if tool_id == "antigravity_app" => {
            let c = AntigravityClientController::new();
            if c.is_running() {
                c.stop(15.0)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_cli_tool_for_client(client: AccountClient, tool_id: &str) -> bool {
    matches!(
        (client, tool_id),
        (AccountClient::Codex, "codex_cli")
            | (AccountClient::Claude, "claude_cli")
            | (AccountClient::Antigravity, "antigravity_cli")
    )
}

fn is_editor_tool(tool_id: &str) -> bool {
    matches!(tool_id, "vscode" | "cursor" | "windsurf" | "trae")
}

fn strip_leading_cli_bin(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    const PREFIXES: [&str; 9] = [
        "codex.exe",
        "codex.cmd",
        "codex",
        "claude.exe",
        "claude.cmd",
        "claude",
        "agy.exe",
        "agy.cmd",
        "agy",
    ];
    for prefix in PREFIXES {
        if !trimmed.is_char_boundary(prefix.len()) {
            continue;
        }
        if trimmed[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let rest = &trimmed[prefix.len()..];
            if rest.is_empty() {
                return None;
            }
            if rest.starts_with(char::is_whitespace) {
                let args = rest.trim();
                return if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
            }
        }
    }
    Some(trimmed.to_string())
}

fn build_native_cli_args(
    client: AccountClient,
    session_id: &str,
    extra_flags: Option<&str>,
) -> (&'static str, String) {
    match client {
        AccountClient::Codex => {
            let args = match extra_flags {
                Some(flags) => format!("resume {} {}", flags, session_id),
                None => format!("resume {}", session_id),
            };
            ("codex", args)
        }
        AccountClient::Claude => {
            let args = match extra_flags {
                Some(flags) => format!("{} --resume {}", flags, session_id),
                None => format!("--resume {}", session_id),
            };
            ("claude", args)
        }
        AccountClient::Antigravity => {
            let args = match extra_flags {
                Some(flags) => format!("{} --conversation {}", flags, session_id),
                None => format!("--conversation {}", session_id),
            };
            ("agy", args)
        }
    }
}

fn build_compact_boot_prompt(prompt_path_ref: &str) -> String {
    let safe_path: String = prompt_path_ref
        .replace('\\', "/")
        .chars()
        .filter(|&c| c != '"' && c != '$' && c != '`' && c != '\r' && c != '\n')
        .collect();
    format!(
        "请先阅读 {} 了解前序会话上下文与当前代码进度，然后继续推进其中的下一步任务。",
        safe_path
    )
}

fn build_antigravity_cli_args(
    boot_prompt: &str,
    project_id: Option<&str>,
    extra_flags: Option<&str>,
) -> (&'static str, String) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(pid) = project_id.map(str::trim).filter(|s| !s.is_empty()) {
        let safe_pid: String = pid
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !safe_pid.is_empty() {
            parts.push(format!("--project {}", safe_pid));
        }
    }
    if let Some(flags) = extra_flags {
        parts.push(flags.to_string());
    }
    parts.push(format!("-i \"{}\"", boot_prompt));
    ("agy", parts.join(" "))
}

fn build_compact_cli_args(
    client: AccountClient,
    boot_prompt: &str,
    extra_flags: Option<&str>,
) -> (&'static str, String) {
    match client {
        AccountClient::Codex => {
            let args = match extra_flags {
                Some(flags) => format!("{} \"{}\"", flags, boot_prompt),
                None => format!("\"{}\"", boot_prompt),
            };
            ("codex", args)
        }
        AccountClient::Claude => {
            let args = match extra_flags {
                Some(flags) => format!("{} \"{}\"", flags, boot_prompt),
                None => format!("\"{}\"", boot_prompt),
            };
            ("claude", args)
        }
        AccountClient::Antigravity => {
            let args = match extra_flags {
                Some(flags) => format!("{} -i \"{}\"", flags, boot_prompt),
                None => format!("-i \"{}\"", boot_prompt),
            };
            ("agy", args)
        }
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
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

fn normalize_dir_for_compare(p: &str) -> String {
    let mut s = normalize_windows_path(p).replace('/', "\\");
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

fn resolve_codex_catalog_path(codex_home: &Path) -> Option<PathBuf> {
    let sqlite_dir = codex_home.join("sqlite");
    for name in ["codex.db", "codex-dev.db"] {
        let p = sqlite_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn detect_codex_provider(codex_home: &Path) -> String {
    let auth_path = codex_home.join("auth.json");
    if let Ok(content) = fs::read_to_string(&auth_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if val
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            {
                let cfg_path = codex_home.join("config.toml");
                if let Ok(cfg) = fs::read_to_string(cfg_path) {
                    for line in cfg.lines() {
                        let t = line.trim();
                        if let Some(rest) = t.strip_prefix("model_provider") {
                            if let Some((_, val_part)) = rest.split_once('=') {
                                let clean = val_part.trim().trim_matches('"').trim_matches('\'');
                                if !clean.is_empty() {
                                    return clean.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(cat) = resolve_codex_catalog_path(codex_home) {
        if let Ok(conn) =
            Connection::open_with_flags(&cat, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            if let Ok(prov) = conn.query_row(
                "SELECT model_provider FROM local_thread_catalog
                 WHERE host_id = 'local' AND model_provider IS NOT NULL AND model_provider <> ''
                 ORDER BY source_updated_at DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            ) {
                if !prov.trim().is_empty() {
                    return prov;
                }
            }
        }
    }
    "openai".to_string()
}

#[derive(Debug, Clone, Default)]
struct CodexProjectIds {
    state_project_id: Option<String>,
    legacy_project_id: Option<String>,
}

fn sqlite_has_table(conn: &Connection, table_name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [table_name],
        |_| Ok(()),
    )
    .is_ok()
}

fn ensure_codex_workspace_project(
    codex_home: &Path,
    backup_root: &Path,
    working_dir: &str,
    warnings: &mut Vec<String>,
) -> CodexProjectIds {
    let mut result = CodexProjectIds::default();
    let target_norm = normalize_dir_for_compare(working_dir);
    if target_norm.is_empty() {
        return result;
    }

    let folder_name = folder_display_name(working_dir);
    let now_local = Local::now();
    let now_ms = Utc::now().timestamp_millis();
    let stamp = now_local.format("%Y%m%d-%H%M%S-%3f").to_string();
    let backup_dir = backup_root.join(format!("codex_ws_{}_{}", stamp, Uuid::new_v4()));

    // 1. 若 state_5.sqlite 已存在（切勿凭空创建无 _sqlx_migrations 的空库），确保 projects + project_roots 存在该工作目录
    let state_path = codex_home.join("state_5.sqlite");
    if state_path.is_file() {
        match (|| -> Result<Option<String>, String> {
            let mut conn = Connection::open(&state_path).map_err(|e| e.to_string())?;
            conn.busy_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| e.to_string())?;
            if !sqlite_has_table(&conn, "projects") || !sqlite_has_table(&conn, "project_roots") {
                return Ok(None);
            }

            let existing_pid: Option<String> = {
                let mut stmt = conn
                    .prepare("SELECT project_id, path FROM project_roots")
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
                let mut found = None;
                while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                    let pid: String = row.get(0).map_err(|e| e.to_string())?;
                    let path_val: String = row.get(1).map_err(|e| e.to_string())?;
                    if normalize_dir_for_compare(&path_val) == target_norm {
                        found = Some(pid);
                        break;
                    }
                }
                found
            };

            if let Some(pid) = existing_pid {
                let _ = fs::create_dir_all(&backup_dir);
                let _ = backup_sqlite(&state_path, &backup_dir.join("state_5.sqlite"));
                let _ = conn.execute(
                    "UPDATE projects SET updated_at_ms = ?1 WHERE id = ?2",
                    rusqlite::params![now_ms, &pid],
                );
                return Ok(Some(pid));
            }

            let _ = fs::create_dir_all(&backup_dir);
            backup_sqlite(&state_path, &backup_dir.join("state_5.sqlite"))?;

            let new_pid = Uuid::new_v4().to_string();
            let next_pos: i64 = conn
                .query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM projects", [], |r| r.get(0))
                .unwrap_or(0);
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO projects (id, name, metadata, position, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, '{}', ?3, ?4, ?4)",
                rusqlite::params![&new_pid, &folder_name, next_pos, now_ms],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO project_roots (project_id, position, path) VALUES (?1, 0, ?2)",
                rusqlite::params![&new_pid, working_dir],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(Some(new_pid))
        })() {
            Ok(pid) => result.state_project_id = pid,
            Err(e) => warnings.push(format!("注册 Codex state_5 工作区失败：{}", e)),
        }
    }

    // 2. 若 .codex-global-state.json 存在，确保 local-projects / project-order / host mapping 有对应记录
    let global_state_path = codex_home.join(".codex-global-state.json");
    if global_state_path.is_file() {
        let _ = fs::create_dir_all(&backup_dir);
        let _ = fs::copy(&global_state_path, backup_dir.join(".codex-global-state.json"));
        match ensure_codex_global_state_project(
            &global_state_path,
            codex_home,
            working_dir,
            &folder_name,
            result.state_project_id.as_deref(),
            now_ms,
        ) {
            Ok(legacy_id) => result.legacy_project_id = Some(legacy_id),
            Err(e) => warnings.push(format!("同步 Codex 侧边栏工作文件夹失败：{}", e)),
        }
    }

    result
}

fn ensure_codex_global_state_project(
    path: &Path,
    codex_home: &Path,
    working_dir: &str,
    folder_name: &str,
    state_project_id: Option<&str>,
    now_ms: i64,
) -> Result<String, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut payload: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let root_obj = payload
        .as_object_mut()
        .ok_or_else(|| ".codex-global-state.json 根节点不是 JSON 对象".to_string())?;

    let target_norm = normalize_dir_for_compare(working_dir);
    let existing_legacy = root_obj
        .get("local-projects")
        .and_then(|v| v.as_object())
        .and_then(|projects| {
            projects.iter().find_map(|(pid, pdata)| {
                let roots = pdata.get("rootPaths")?.as_array()?;
                let found = roots.iter().any(|r| {
                    r.as_str()
                        .map(|s| normalize_dir_for_compare(s) == target_norm)
                        .unwrap_or(false)
                });
                found.then(|| pid.clone())
            })
        });

    let legacy_pid = if let Some(pid) = existing_legacy {
        if let Some(proj_obj) = root_obj
            .get_mut("local-projects")
            .and_then(|v| v.as_object_mut())
            .and_then(|m| m.get_mut(&pid))
            .and_then(|v| v.as_object_mut())
        {
            proj_obj.insert("updatedAt".to_string(), serde_json::json!(now_ms));
        }
        pid
    } else {
        let new_legacy_pid = Uuid::new_v4().to_string();
        if !root_obj.contains_key("local-projects") {
            root_obj.insert("local-projects".to_string(), serde_json::json!({}));
        }
        if let Some(projects) = root_obj
            .get_mut("local-projects")
            .and_then(|v| v.as_object_mut())
        {
            projects.insert(
                new_legacy_pid.clone(),
                serde_json::json!({
                    "id": new_legacy_pid,
                    "name": folder_name,
                    "rootPaths": [working_dir],
                    "createdAt": now_ms,
                    "updatedAt": now_ms
                }),
            );
        }
        if !root_obj.contains_key("project-order") {
            root_obj.insert("project-order".to_string(), serde_json::json!([]));
        }
        if let Some(order) = root_obj
            .get_mut("project-order")
            .and_then(|v| v.as_array_mut())
        {
            if !order.iter().any(|v| v.as_str() == Some(&new_legacy_pid)) {
                order.insert(0, serde_json::json!(new_legacy_pid));
            }
        }
        new_legacy_pid
    };

    if let Some(spid) = state_project_id {
        if !root_obj.contains_key("app-server-project-id-by-legacy-project-id-by-host") {
            root_obj.insert(
                "app-server-project-id-by-legacy-project-id-by-host".to_string(),
                serde_json::json!({}),
            );
        }
        if let Some(by_host) = root_obj
            .get_mut("app-server-project-id-by-legacy-project-id-by-host")
            .and_then(|v| v.as_object_mut())
        {
            let codex_home_norm = clean_working_dir_path(&codex_home.to_string_lossy());
            let host_key = format!("local:{}", codex_home_norm);
            let existing_host_key = by_host
                .keys()
                .find(|k| k.eq_ignore_ascii_case(&host_key))
                .cloned()
                .unwrap_or(host_key);
            if !by_host.contains_key(&existing_host_key) {
                by_host.insert(existing_host_key.clone(), serde_json::json!({}));
            }
            if let Some(map_obj) = by_host
                .get_mut(&existing_host_key)
                .and_then(|v| v.as_object_mut())
            {
                map_obj.insert(legacy_pid.clone(), serde_json::json!(spid));
            }
        }
    }

    let serialized = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    write_atomic(path, &serialized)?;
    Ok(legacy_pid)
}

fn ensure_claude_workspace_preset_in_dir(
    claude_home: &Path,
    claude_data_dir: Option<&Path>,
    working_dir: &str,
) -> Result<Option<String>, String> {
    let proj_dir = find_claude_project_dir_for_cwd(claude_home, Some(working_dir));
    fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("创建 Claude 项目目录失败 {}: {}", proj_dir.display(), e))?;

    let Some(data_dir) = claude_data_dir.filter(|p| p.is_dir()) else {
        return Ok(None);
    };

    let target_norm = normalize_dir_for_compare(working_dir);
    let folder_name = folder_display_name(working_dir);
    let now_ms = Utc::now().timestamp_millis();
    let mut matched_or_created_id: Option<String> = None;

    let entries = fs::read_dir(data_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let pkg_dir = entry.path();
        if !pkg_dir.is_dir() {
            continue;
        }
        let presets_path = pkg_dir.join("ccd-session-presets.json");
        if !presets_path.is_file() {
            continue;
        }

        let raw = fs::read_to_string(&presets_path).map_err(|e| e.to_string())?;
        let mut payload: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(by_account) = payload.get_mut("byAccount").and_then(|v| v.as_object_mut()) else {
            continue;
        };

        let mut modified = false;
        for (_acct_id, acct_val) in by_account.iter_mut() {
            let Some(acct_obj) = acct_val.as_object_mut() else {
                continue;
            };
            if !acct_obj.contains_key("presets") {
                acct_obj.insert("presets".to_string(), serde_json::json!([]));
            }
            let Some(presets) = acct_obj.get_mut("presets").and_then(|v| v.as_array_mut()) else {
                continue;
            };

            let existing_idx = presets.iter().position(|item| {
                let kind = item
                    .get("target")
                    .and_then(|t| t.get("kind"))
                    .and_then(|v| v.as_str());
                let cwd = item
                    .get("target")
                    .and_then(|t| t.get("cwd"))
                    .and_then(|v| v.as_str());
                matches!(kind, Some("local"))
                    && cwd
                        .map(|s| normalize_dir_for_compare(s) == target_norm)
                        .unwrap_or(false)
            });

            if let Some(idx) = existing_idx {
                if let Some(obj) = presets[idx].as_object_mut() {
                    if matched_or_created_id.is_none() {
                        matched_or_created_id =
                            obj.get("id").and_then(|v| v.as_str()).map(str::to_string);
                    }
                    obj.insert("lastUsedAt".to_string(), serde_json::json!(now_ms));
                    modified = true;
                }
            } else {
                let new_id = Uuid::new_v4().to_string();
                if matched_or_created_id.is_none() {
                    matched_or_created_id = Some(new_id.clone());
                }
                presets.insert(
                    0,
                    serde_json::json!({
                        "id": new_id,
                        "name": folder_name,
                        "target": {
                            "kind": "local",
                            "cwd": working_dir,
                            "additionalFolders": []
                        },
                        "settings": {},
                        "createdAt": now_ms,
                        "lastUsedAt": now_ms
                    }),
                );
                modified = true;
            }
        }

        if modified {
            let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
            write_atomic(&presets_path, &bytes)?;
        }
    }

    Ok(matched_or_created_id)
}

fn encode_antigravity_folder_uri(dir: &str) -> String {
    let mut normalized = clean_working_dir_path(dir).replace('\\', "/");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        normalized = format!("{}{}", normalized[..1].to_lowercase(), &normalized[1..]);
    }
    format!(
        "file:///{}",
        percent_encode_path(normalized.trim_start_matches('/'))
    )
}

fn folder_uri_matches_dir(uri: &str, working_dir: &str) -> bool {
    let target_norm = normalize_dir_for_compare(working_dir);
    let decoded = crate::util::percent_decode(uri);
    let stripped = decoded
        .strip_prefix("file:///")
        .or_else(|| decoded.strip_prefix("file://"))
        .unwrap_or(&decoded);
    normalize_dir_for_compare(stripped) == target_norm
}

pub fn ensure_antigravity_workspace_project(
    gemini_home: &Path,
    app_storage_path: Option<&Path>,
    working_dir: &str,
) -> Result<String, String> {
    let projects_dir = gemini_home.join("config").join("projects");
    fs::create_dir_all(&projects_dir)
        .map_err(|e| format!("创建 Antigravity projects 目录失败 {}: {}", projects_dir.display(), e))?;

    let mut matched_id: Option<String> = None;
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file()
                || p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("json"))
                    != Some(true)
            {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&p) else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(resources) = val
                .get("projectResources")
                .and_then(|r| r.get("resources"))
                .and_then(|r| r.as_array())
            else {
                continue;
            };
            if resources.iter().any(|res| {
                res.get("folderUri")
                    .and_then(|u| u.as_str())
                    .map(|u| folder_uri_matches_dir(u, working_dir))
                    .unwrap_or(false)
            }) {
                if let Some(id) = val.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    matched_id = Some(id.to_string());
                    break;
                }
            }
        }
    }

    let project_id = if let Some(existing) = matched_id {
        existing
    } else {
        let new_id = Uuid::new_v4().to_string();
        let folder_name = folder_display_name(working_dir);
        let folder_uri = encode_antigravity_folder_uri(working_dir);
        let project_json = serde_json::json!({
            "id": new_id,
            "name": folder_name,
            "projectResources": {
                "resources": [
                    { "folderUri": folder_uri }
                ]
            },
            "permissionGrants": {
                "v2Migrated": true
            },
            "settings": {},
            "isWorkspaceOnly": false
        });
        let proj_path = projects_dir.join(format!("{}.json", new_id));
        let bytes = serde_json::to_vec_pretty(&project_json).map_err(|e| e.to_string())?;
        write_atomic(&proj_path, &bytes)?;
        new_id
    };

    if let Some(storage_path) = app_storage_path.filter(|p| p.is_file()) {
        if let Ok(raw) = fs::read_to_string(storage_path) {
            if let Ok(mut storage_val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(obj) = storage_val.as_object_mut() {
                    obj.insert(
                        "new-convo-last-selected-project".to_string(),
                        serde_json::json!(project_id),
                    );
                    obj.insert(
                        "new-convo-magic-workspace-selected".to_string(),
                        serde_json::json!("false"),
                    );
                    let mut order_ids: Vec<String> = obj
                        .get("projectsOrder")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                        .unwrap_or_default();
                    if !order_ids.iter().any(|id| id == &project_id) {
                        order_ids.insert(0, project_id.clone());
                        if let Ok(order_str) = serde_json::to_string(&order_ids) {
                            obj.insert(
                                "projectsOrder".to_string(),
                                serde_json::json!(order_str),
                            );
                        }
                    }
                    if let Ok(bytes) = serde_json::to_vec(&storage_val) {
                        let _ = write_atomic(storage_path, &bytes);
                    }
                }
            }
        }
    }

    Ok(project_id)
}

pub fn inject_codex_session(
    codex_home: &Path,
    backup_root: &Path,
    session_id: &str,
    title: &str,
    project_path: Option<&str>,
    messages: &[CanonicalMessage],
    warnings: &mut Vec<String>,
) -> Result<PathBuf, String> {
    inject_codex_session_with_projects(
        codex_home,
        backup_root,
        session_id,
        title,
        project_path,
        &CodexProjectIds::default(),
        messages,
        warnings,
    )
}

struct CodexHistoryTurnDraft {
    turn_id: String,
    start_ordinal: i64,
    end_ordinal: i64,
    start_byte_offset: usize,
    end_byte_offset: usize,
    started_sec: i64,
    completed_sec: i64,
    first_user_item_id: Option<String>,
    final_agent_item_id: Option<String>,
    items: Vec<CodexHistoryItemDraft>,
}

struct CodexHistoryItemDraft {
    item_id: String,
    rollout_ordinal: i64,
    created_at_ms: i64,
    item_type: String,
    item_json: String,
}

#[allow(clippy::too_many_arguments)]
fn inject_codex_session_with_projects(
    codex_home: &Path,
    backup_root: &Path,
    session_id: &str,
    title: &str,
    project_path: Option<&str>,
    project_ids: &CodexProjectIds,
    messages: &[CanonicalMessage],
    warnings: &mut Vec<String>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(codex_home)
        .map_err(|e| format!("无法创建 Codex 目录 {}: {}", codex_home.display(), e))?;
    let now_local = Local::now();
    let now_utc = Utc::now();
    let rollout_dir = codex_home
        .join("sessions")
        .join(now_local.format("%Y").to_string())
        .join(now_local.format("%m").to_string())
        .join(now_local.format("%d").to_string());
    fs::create_dir_all(&rollout_dir)
        .map_err(|e| format!("创建 Codex sessions 目录失败: {}", e))?;

    let rollout_name = format!(
        "rollout-{}-{}.jsonl",
        now_local.format("%Y-%m-%dT%H-%M-%S"),
        session_id
    );
    let rollout_path = rollout_dir.join(rollout_name);
    let cwd_str = project_path.unwrap_or("").trim();
    let provider = detect_codex_provider(codex_home);
    let history_db_path = codex_home.join("thread_history_1.sqlite");
    let use_paginated = history_db_path.is_file();

    let first_ts = messages
        .first()
        .map(|m| m.created_at)
        .unwrap_or(now_utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let roots: Vec<&str> = if cwd_str.is_empty() {
        Vec::new()
    } else {
        vec![cwd_str]
    };

    let mut rollout_lines = Vec::new();
    let mut byte_cursor: usize = 0;
    let mut ordinal_cursor: i64 = 0;

    let push_line = |line: String,
                     lines: &mut Vec<String>,
                     bytes: &mut usize,
                     ord: &mut i64|
     -> (i64, usize, usize) {
        let start_ord = *ord;
        let start_byte = *bytes;
        let len_with_nl = line.len() + 1;
        lines.push(line);
        *bytes += len_with_nl;
        *ord += 1;
        (start_ord, start_byte, *bytes)
    };

    push_line(
        serde_json::json!({
            "timestamp": now_utc.to_rfc3339_opts(SecondsFormat::Millis, true),
            "type": "session_meta",
            "payload": {
                "id": session_id,
                "session_id": session_id,
                "timestamp": first_ts,
                "cwd": cwd_str,
                "runtime_workspace_roots": roots,
                "originator": "Context Vault",
                "source": "vscode",
                "thread_source": "user",
                "model_provider": provider,
                "history_mode": if use_paginated { "paginated" } else { "legacy" }
            }
        })
        .to_string(),
        &mut rollout_lines,
        &mut byte_cursor,
        &mut ordinal_cursor,
    );

    let mut turns: Vec<CodexHistoryTurnDraft> = Vec::new();
    for msg in messages {
        if !matches!(msg.role, MessageRole::User | MessageRole::Assistant) {
            continue;
        }
        let ts = msg.created_at.to_rfc3339_opts(SecondsFormat::Millis, true);
        let msg_sec = msg.created_at.timestamp();
        let msg_ms = msg.created_at.timestamp_millis();
        let msg_item_id = format!("msg_{}", Uuid::new_v4());

        if msg.role == MessageRole::User {
            let (resp_ord, resp_start, _) = push_line(
                serde_json::json!({
                    "timestamp": ts,
                    "type": "response_item",
                    "payload": {
                        "type": "message",
                        "id": msg_item_id,
                        "role": "user",
                        "content": [{ "type": "input_text", "text": msg.content }]
                    }
                })
                .to_string(),
                &mut rollout_lines,
                &mut byte_cursor,
                &mut ordinal_cursor,
            );
            let (ev_ord, _, ev_end) = push_line(
                serde_json::json!({
                    "timestamp": ts,
                    "type": "event_msg",
                    "payload": {
                        "type": "user_message",
                        "message": msg.content
                    }
                })
                .to_string(),
                &mut rollout_lines,
                &mut byte_cursor,
                &mut ordinal_cursor,
            );

            let item_json = serde_json::json!({
                "type": "userMessage",
                "id": msg_item_id,
                "clientId": Uuid::new_v4().to_string(),
                "content": [{ "type": "text", "text": msg.content, "text_elements": [] }]
            })
            .to_string();
            let item = CodexHistoryItemDraft {
                item_id: msg_item_id.clone(),
                rollout_ordinal: resp_ord,
                created_at_ms: msg_ms,
                item_type: "userMessage".to_string(),
                item_json,
            };

            turns.push(CodexHistoryTurnDraft {
                turn_id: Uuid::new_v4().to_string(),
                start_ordinal: resp_ord,
                end_ordinal: ev_ord,
                start_byte_offset: resp_start,
                end_byte_offset: ev_end,
                started_sec: msg_sec,
                completed_sec: msg_sec,
                first_user_item_id: Some(msg_item_id),
                final_agent_item_id: None,
                items: vec![item],
            });
        } else if msg.role == MessageRole::Assistant {
            let (resp_ord, resp_start, _) = push_line(
                serde_json::json!({
                    "timestamp": ts,
                    "type": "response_item",
                    "payload": {
                        "type": "message",
                        "status": "completed",
                        "id": msg_item_id,
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": msg.content }],
                        "phase": "final_answer"
                    }
                })
                .to_string(),
                &mut rollout_lines,
                &mut byte_cursor,
                &mut ordinal_cursor,
            );
            let (ev_ord, _, ev_end) = push_line(
                serde_json::json!({
                    "timestamp": ts,
                    "type": "event_msg",
                    "payload": {
                        "type": "agent_message",
                        "message": msg.content,
                        "phase": "final_answer"
                    }
                })
                .to_string(),
                &mut rollout_lines,
                &mut byte_cursor,
                &mut ordinal_cursor,
            );

            let item_json = serde_json::json!({
                "type": "agentMessage",
                "moderation": serde_json::Value::Null,
                "id": msg_item_id,
                "text": msg.content,
                "phase": "final_answer",
                "memoryCitation": serde_json::Value::Null,
                "delivery": serde_json::Value::Null,
                "questions": serde_json::Value::Null
            })
            .to_string();
            let item = CodexHistoryItemDraft {
                item_id: msg_item_id.clone(),
                rollout_ordinal: resp_ord,
                created_at_ms: msg_ms,
                item_type: "agentMessage".to_string(),
                item_json,
            };

            if let Some(last_turn) = turns.last_mut() {
                last_turn.end_ordinal = ev_ord;
                last_turn.end_byte_offset = ev_end;
                last_turn.completed_sec = msg_sec;
                last_turn.final_agent_item_id = Some(msg_item_id);
                last_turn.items.push(item);
            } else {
                turns.push(CodexHistoryTurnDraft {
                    turn_id: Uuid::new_v4().to_string(),
                    start_ordinal: resp_ord,
                    end_ordinal: ev_ord,
                    start_byte_offset: resp_start,
                    end_byte_offset: ev_end,
                    started_sec: msg_sec,
                    completed_sec: msg_sec,
                    first_user_item_id: None,
                    final_agent_item_id: Some(msg_item_id),
                    items: vec![item],
                });
            }
        }
    }

    let rollout_body = rollout_lines.join("\n") + "\n";
    write_atomic(&rollout_path, rollout_body.as_bytes())?;

    // 2. 写入 session_index.jsonl
    let session_index_path = codex_home.join("session_index.jsonl");
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&session_index_path)
    {
        let entry = serde_json::json!({
            "id": session_id,
            "thread_name": title,
        });
        let _ = writeln!(f, "{}", entry);
    }

    // 3. 写入 paginated 历史至 thread_history_1.sqlite（如已存在）
    let mut history_mode_value = "legacy";
    if use_paginated {
        match upsert_codex_thread_history_paginated(
            &history_db_path,
            session_id,
            &turns,
            byte_cursor,
            ordinal_cursor,
        ) {
            Ok(true) => history_mode_value = "paginated",
            Ok(false) => {}
            Err(e) => warnings.push(format!(
                "写入 Codex thread_history_1 失败（CLI --resume 仍可用）：{}",
                e
            )),
        }
    }

    // 4. 安全备份并仅在 state_5.sqlite 已存在时向上写入本条会话
    let stamp = now_local.format("%Y%m%d-%H%M%S-%3f").to_string();
    let backup_dir = backup_root.join(format!("codex_handoff_{}_{}", stamp, Uuid::new_v4()));
    let _ = fs::create_dir_all(&backup_dir);

    let first_user_msg = messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .map(|m| m.content.trim().to_string())
        .unwrap_or_else(|| title.to_string());
    let created_sec = messages
        .first()
        .map(|m| m.created_at.timestamp())
        .unwrap_or_else(|| now_utc.timestamp());
    let created_ms = created_sec * 1000;
    let now_sec = now_utc.timestamp();
    let now_ms = now_utc.timestamp_millis();
    let rollout_path_str = rollout_path.to_string_lossy().to_string();

    let state_path = codex_home.join("state_5.sqlite");
    if state_path.is_file() {
        backup_sqlite(&state_path, &backup_dir.join("state_5.sqlite"))?;
        let state_conn =
            Connection::open(&state_path).map_err(|e| format!("打开 Codex state_5 失败: {}", e))?;
        state_conn
            .busy_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        if sqlite_has_table(&state_conn, "threads") {
            upsert_codex_state_thread(
                &state_conn,
                session_id,
                &rollout_path_str,
                cwd_str,
                title,
                &first_user_msg,
                &provider,
                project_ids.state_project_id.as_deref(),
                history_mode_value,
                created_sec,
                now_sec,
                created_ms,
                now_ms,
            )?;
        }
    }

    if let Some(catalog_path) = resolve_codex_catalog_path(codex_home) {
        let cname = catalog_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("codex.db"));
        if let Err(e) = backup_sqlite(&catalog_path, &backup_dir.join(cname)) {
            warnings.push(format!("备份 Codex catalog 失败（仍已写入 rollout）：{}", e));
        } else if let Err(e) = upsert_codex_catalog_entry(
            &catalog_path,
            session_id,
            title,
            cwd_str,
            &provider,
            project_ids.state_project_id.as_deref(),
            created_sec as f64,
            now_sec as f64,
        ) {
            warnings.push(format!("同步 Codex catalog 条目失败（CLI --resume 仍可用）：{}", e));
        }
    }

    let global_state_path = codex_home.join(".codex-global-state.json");
    if global_state_path.is_file() {
        let _ = fs::copy(&global_state_path, backup_dir.join(".codex-global-state.json"));
        if let Err(e) = update_codex_global_state(
            &global_state_path,
            session_id,
            cwd_str,
            project_ids.legacy_project_id.as_deref(),
        ) {
            warnings.push(format!("更新 Codex 侧边栏状态失败：{}", e));
        }
    }

    Ok(rollout_path)
}

fn upsert_codex_thread_history_paginated(
    history_db_path: &Path,
    session_id: &str,
    turns: &[CodexHistoryTurnDraft],
    next_byte_offset: usize,
    next_ordinal: i64,
) -> Result<bool, String> {
    let mut conn = Connection::open(history_db_path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    if !sqlite_has_table(&conn, "thread_turns")
        || !sqlite_has_table(&conn, "thread_items")
        || !sqlite_has_table(&conn, "thread_history_projection_state")
    {
        return Ok(false);
    }

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for turn in turns {
        let duration_ms = ((turn.completed_sec - turn.started_sec).max(0)) * 1000;
        tx.execute(
            "INSERT OR REPLACE INTO thread_turns (
                thread_id, turn_id, rollout_ordinal, status, error_json,
                started_at, completed_at, duration_ms, first_user_item_id,
                final_agent_item_id, rollout_byte_offset, rollout_end_ordinal,
                rollout_end_byte_offset, root_turn_id
             ) VALUES (?1, ?2, ?3, 'completed', NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?2)",
            rusqlite::params![
                session_id,
                &turn.turn_id,
                turn.start_ordinal,
                turn.started_sec,
                turn.completed_sec,
                duration_ms,
                &turn.first_user_item_id,
                &turn.final_agent_item_id,
                turn.start_byte_offset as i64,
                turn.end_ordinal,
                turn.end_byte_offset as i64,
            ],
        )
        .map_err(|e| e.to_string())?;

        for item in &turn.items {
            tx.execute(
                "INSERT OR REPLACE INTO thread_items (
                    thread_id, turn_id, item_id, rollout_ordinal, created_at_ms,
                    item_json, item_type, updated_at_ordinal, started_at_ms, completed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?5, ?5)",
                rusqlite::params![
                    session_id,
                    &turn.turn_id,
                    &item.item_id,
                    item.rollout_ordinal,
                    item.created_at_ms,
                    &item.item_json,
                    &item.item_type,
                    item.rollout_ordinal,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    tx.execute(
        "INSERT OR REPLACE INTO thread_history_projection_state (
            thread_id, next_rollout_byte_offset, next_rollout_ordinal
         ) VALUES (?1, ?2, ?3)",
        rusqlite::params![session_id, next_byte_offset as i64, next_ordinal],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn upsert_codex_state_thread(
    conn: &Connection,
    session_id: &str,
    rollout_path: &str,
    cwd_str: &str,
    title: &str,
    first_user_msg: &str,
    provider: &str,
    state_project_id: Option<&str>,
    history_mode: &str,
    created_sec: i64,
    now_sec: i64,
    created_ms: i64,
    now_ms: i64,
) -> Result<(), String> {
    let mut cols_info = Vec::new();
    let mut stmt = conn
        .prepare("PRAGMA table_info(threads)")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2).unwrap_or_default(),
                r.get::<_, i64>(3).unwrap_or(0),
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        cols_info.push(row);
    }

    let mut col_names: Vec<String> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    for (name, col_type, notnull, dflt) in cols_info {
        let val = match name.as_str() {
            "id" => Some(SqlValue::Text(session_id.to_string())),
            "rollout_path" => Some(SqlValue::Text(rollout_path.to_string())),
            "created_at" => Some(SqlValue::Integer(created_sec)),
            "updated_at" | "recency_at" => Some(SqlValue::Integer(now_sec)),
            "created_at_ms" => Some(SqlValue::Integer(created_ms)),
            "updated_at_ms" | "recency_at_ms" => Some(SqlValue::Integer(now_ms)),
            "source" => Some(SqlValue::Text("vscode".to_string())),
            "thread_source" => Some(SqlValue::Text("user".to_string())),
            "model_provider" => Some(SqlValue::Text(provider.to_string())),
            "cwd" => Some(SqlValue::Text(cwd_str.to_string())),
            "title" | "name" => Some(SqlValue::Text(title.to_string())),
            "first_user_message" | "preview" => Some(SqlValue::Text(first_user_msg.to_string())),
            "sandbox_policy" => Some(SqlValue::Text("{\"type\":\"disabled\"}".to_string())),
            "approval_mode" => Some(SqlValue::Text("never".to_string())),
            "tokens_used" | "archived" | "is_pinned" => Some(SqlValue::Integer(0)),
            "has_user_event" => Some(SqlValue::Integer(1)),
            "history_mode" => Some(SqlValue::Text(history_mode.to_string())),
            "originator" => Some(SqlValue::Text("Context Vault".to_string())),
            "memory_mode" => Some(SqlValue::Text("enabled".to_string())),
            "cli_version" => Some(SqlValue::Text(String::new())),
            "project_id" => state_project_id.map(|pid| SqlValue::Text(pid.to_string())),
            _ => {
                if notnull != 0 && dflt.is_none() {
                    let upper = col_type.to_ascii_uppercase();
                    if upper.contains("INT") || upper.contains("BOOL") {
                        Some(SqlValue::Integer(0))
                    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
                        Some(SqlValue::Real(0.0))
                    } else {
                        Some(SqlValue::Text(String::new()))
                    }
                } else {
                    None
                }
            }
        };
        if let Some(v) = val {
            col_names.push(name);
            values.push(v);
        }
    }

    if col_names.is_empty() {
        return Err("Codex threads 表无有效列".to_string());
    }

    let placeholders = (1..=col_names.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT OR REPLACE INTO threads ({}) VALUES ({})",
        col_names.join(", "),
        placeholders
    );
    let param_refs: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
    conn.execute(&sql, param_refs.as_slice())
        .map_err(|e| format!("写入 Codex threads 失败: {}", e))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert_codex_catalog_entry(
    catalog_path: &Path,
    session_id: &str,
    title: &str,
    cwd_str: &str,
    provider: &str,
    state_project_id: Option<&str>,
    created_sec: f64,
    now_sec: f64,
) -> Result<(), String> {
    let conn = Connection::open(catalog_path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;

    if !sqlite_has_table(&conn, "local_thread_catalog") {
        return Ok(());
    }

    if sqlite_has_table(&conn, "local_thread_catalog_hosts") {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO local_thread_catalog_hosts(host_id, host_kind) VALUES('local', 'local')",
            [],
        );
    }

    let max_seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(observation_sequence), 0) FROM local_thread_catalog WHERE host_id = 'local'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let next_seq = max_seq + 1;

    let mut cols_info = Vec::new();
    let mut stmt = conn
        .prepare("PRAGMA table_info(local_thread_catalog)")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2).unwrap_or_default(),
                r.get::<_, i64>(3).unwrap_or(0),
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        cols_info.push(row);
    }

    let mut col_names: Vec<String> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    for (name, col_type, notnull, dflt) in cols_info {
        let val = match name.as_str() {
            "host_id" => Some(SqlValue::Text("local".to_string())),
            "thread_id" => Some(SqlValue::Text(session_id.to_string())),
            "display_title" => Some(SqlValue::Text(title.to_string())),
            "source_created_at" => Some(SqlValue::Real(created_sec)),
            "source_updated_at" | "source_recency_at" => Some(SqlValue::Real(now_sec)),
            "cwd" => Some(SqlValue::Text(cwd_str.to_string())),
            "source_kind" => Some(SqlValue::Text("vscode".to_string())),
            "model_provider" => Some(SqlValue::Text(provider.to_string())),
            "observation_sequence" => Some(SqlValue::Integer(next_seq)),
            "missing_candidate" | "pending_observed_title" => Some(SqlValue::Integer(0)),
            "thread_source" => Some(SqlValue::Text("user".to_string())),
            "project_id" => state_project_id.map(|pid| SqlValue::Text(pid.to_string())),
            _ => {
                if notnull != 0 && dflt.is_none() {
                    let upper = col_type.to_ascii_uppercase();
                    if upper.contains("INT") || upper.contains("BOOL") {
                        Some(SqlValue::Integer(0))
                    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
                        Some(SqlValue::Real(0.0))
                    } else {
                        Some(SqlValue::Text(String::new()))
                    }
                } else {
                    None
                }
            }
        };
        if let Some(v) = val {
            col_names.push(name);
            values.push(v);
        }
    }

    let placeholders = (1..=col_names.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT OR REPLACE INTO local_thread_catalog ({}) VALUES ({})",
        col_names.join(", "),
        placeholders
    );
    let param_refs: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
    conn.execute(&sql, param_refs.as_slice())
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_codex_global_state(
    path: &Path,
    session_id: &str,
    cwd_str: &str,
    known_legacy_pid: Option<&str>,
) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut payload: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let Some(root_obj) = payload.as_object_mut() else {
        return Ok(());
    };

    let target_norm = normalize_dir_for_compare(cwd_str);
    let matched_pid: Option<String> = known_legacy_pid
        .map(str::to_string)
        .or_else(|| {
            if target_norm.is_empty() {
                return None;
            }
            root_obj
                .get("local-projects")
                .and_then(|v| v.as_object())
                .and_then(|projects| {
                    projects.iter().find_map(|(pid, pdata)| {
                        let roots = pdata.get("rootPaths")?.as_array()?;
                        let found = roots.iter().any(|r| {
                            r.as_str()
                                .map(|s| normalize_dir_for_compare(s) == target_norm)
                                .unwrap_or(false)
                        });
                        found.then(|| pid.clone())
                    })
                })
        });

    if let Some(pid) = matched_pid {
        if !root_obj.contains_key("thread-project-assignments") {
            root_obj.insert(
                "thread-project-assignments".to_string(),
                serde_json::json!({}),
            );
        }
        if let Some(assigns) = root_obj
            .get_mut("thread-project-assignments")
            .and_then(|v| v.as_object_mut())
        {
            assigns.insert(
                session_id.to_string(),
                serde_json::json!({
                    "projectKind": "local",
                    "projectId": pid
                }),
            );
        }
    } else if let Some(chat_ids) = root_obj
        .get_mut("projectless-thread-ids")
        .and_then(|v| v.as_array_mut())
    {
        if !chat_ids.iter().any(|v| v.as_str() == Some(session_id)) {
            chat_ids.insert(0, serde_json::json!(session_id));
        }
    }

    let serialized = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    write_atomic(path, &serialized)
}

fn find_claude_project_dir_for_cwd(claude_home: &Path, project_path: Option<&str>) -> PathBuf {
    let projects_dir = claude_home.join("projects");
    let target = project_path.unwrap_or("").trim();
    if target.is_empty() {
        return projects_dir.join("default");
    }

    let canonical = projects_dir.join(ClaudePublisher::project_dir_name(target));
    if canonical.is_dir() {
        return canonical;
    }

    let target_norm = normalize_dir_for_compare(target);
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let Ok(sess_entries) = fs::read_dir(&p) else {
                continue;
            };
            for sp in sess_entries.flatten().map(|e| e.path()) {
                if !sp.is_file()
                    || sp.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("jsonl")) != Some(true)
                {
                    continue;
                }
                if let Ok(file) = File::open(&sp) {
                    let reader = BufReader::new(file);
                    if let Some(cwd) = reader.lines().take(50).flatten().find_map(|line| {
                        serde_json::from_str::<serde_json::Value>(&line)
                            .ok()?
                            .get("cwd")?
                            .as_str()
                            .map(str::to_string)
                    }) {
                        if normalize_dir_for_compare(&cwd) == target_norm {
                            return p;
                        }
                    }
                }
                break;
            }
        }
    }

    canonical
}

pub fn inject_claude_session(
    claude_home: &Path,
    backup_root: &Path,
    session_id: &str,
    title: &str,
    project_path: Option<&str>,
    messages: &[CanonicalMessage],
) -> Result<PathBuf, String> {
    fs::create_dir_all(claude_home)
        .map_err(|e| format!("无法创建 Claude 目录 {}: {}", claude_home.display(), e))?;
    let effective_cwd: Option<String> = project_path
        .map(clean_working_dir_path)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            dirs::home_dir()
                .map(|h| clean_working_dir_path(&h.to_string_lossy()))
                .filter(|s| !s.is_empty())
        });
    let proj_dir = find_claude_project_dir_for_cwd(claude_home, effective_cwd.as_deref());
    fs::create_dir_all(&proj_dir)
        .map_err(|e| format!("创建 Claude 项目目录失败 {}: {}", proj_dir.display(), e))?;

    let history_file = claude_home.join("history.jsonl");
    if history_file.is_file() {
        let backup_dir = backup_root.join("claude").join(format!(
            "handoff_{}_{}",
            Local::now().format("%Y%m%d_%H%M%S"),
            Uuid::new_v4()
        ));
        let _ = fs::create_dir_all(&backup_dir);
        let _ = fs::copy(&history_file, backup_dir.join("history.jsonl"));
    }

    let cwd_str = effective_cwd.as_deref().unwrap_or("");
    let sess_jsonl_path = proj_dir.join(format!("{}.jsonl", session_id));
    let mut lines = Vec::new();
    let mut parent_uuid: Option<String> = None;

    for (idx, msg) in messages.iter().enumerate() {
        let msg_uuid = Uuid::parse_str(&msg.id)
            .map(|_| msg.id.clone())
            .unwrap_or_else(|_| Uuid::new_v4().to_string());
        let ts = msg.created_at.to_rfc3339_opts(SecondsFormat::Millis, true);

        if msg.role == MessageRole::User {
            lines.push(
                serde_json::json!({
                    "type": "user",
                    "isSidechain": false,
                    "userType": "external",
                    "message": {
                        "role": "user",
                        "content": msg.content
                    },
                    "uuid": msg_uuid,
                    "sessionId": session_id,
                    "parentUuid": parent_uuid,
                    "timestamp": ts,
                    "cwd": cwd_str
                })
                .to_string(),
            );
            parent_uuid = Some(msg_uuid);
        } else if msg.role == MessageRole::Assistant {
            lines.push(
                serde_json::json!({
                    "type": "assistant",
                    "isSidechain": false,
                    "userType": "external",
                    "requestId": format!("req_{}_{}", session_id, idx),
                    "uuid": msg_uuid,
                    "sessionId": session_id,
                    "parentUuid": parent_uuid,
                    "cwd": cwd_str,
                    "timestamp": ts,
                    "message": {
                        "id": format!("msg_{}", msg_uuid.replace('-', "")),
                        "type": "message",
                        "role": "assistant",
                        "model": msg.model.clone().unwrap_or_else(|| "<synthetic>".to_string()),
                        "content": [{ "type": "text", "text": msg.content }]
                    }
                })
                .to_string(),
            );
            parent_uuid = Some(msg_uuid);
        }
    }

    lines.push(
        serde_json::json!({
            "type": "custom-title",
            "customTitle": title,
            "sessionId": session_id,
        })
        .to_string(),
    );
    lines.push(
        serde_json::json!({
            "type": "agent-name",
            "agentName": title,
            "sessionId": session_id,
        })
        .to_string(),
    );

    let jsonl_body = lines.join("\n") + "\n";
    write_atomic(&sess_jsonl_path, jsonl_body.as_bytes())?;

    let custom_title_path = proj_dir.join(session_id).join("custom-title.json");
    let title_bytes = serde_json::to_vec_pretty(&serde_json::json!({ "customTitle": title }))
        .map_err(|e| e.to_string())?;
    write_atomic(&custom_title_path, &title_bytes)?;

    let mut history_lines = Vec::new();
    if history_file.is_file() {
        if let Ok(content) = fs::read_to_string(&history_file) {
            for l in content.lines() {
                if !l.trim().is_empty() {
                    history_lines.push(l.to_string());
                }
            }
        }
    }
    history_lines.push(
        serde_json::json!({
            "display": title,
            "project": cwd_str,
            "sessionId": session_id,
            "timestamp": Utc::now().timestamp_millis()
        })
        .to_string(),
    );
    let history_body = history_lines.join("\n") + "\n";
    write_atomic(&history_file, history_body.as_bytes())?;

    Ok(sess_jsonl_path)
}

fn ensure_git_info_exclude(working_dir: &Path) {
    let git_dir = working_dir.join(".git");
    if !git_dir.is_dir() {
        return;
    }
    let info_dir = git_dir.join("info");
    if fs::create_dir_all(&info_dir).is_err() {
        return;
    }
    let exclude_file = info_dir.join("exclude");
    let existing = fs::read_to_string(&exclude_file).unwrap_or_default();
    if existing
        .lines()
        .any(|l| l.trim() == ".context-vault/" || l.trim() == ".context-vault")
    {
        return;
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(".context-vault/\n");
    let _ = fs::write(&exclude_file, updated);
}

fn run_git_capture(dir: &Path, args: &[&str], max_lines: usize) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().take(max_lines).collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn gather_git_context(working_dir: &Path) -> Option<String> {
    if !working_dir.join(".git").exists() {
        return None;
    }
    let status = run_git_capture(working_dir, &["status", "--short", "--branch"], 40);
    let diff = run_git_capture(working_dir, &["diff", "--stat"], 40);
    if status.is_none() && diff.is_none() {
        return None;
    }
    let mut out = String::new();
    if let Some(s) = status {
        out.push_str("### `git status --short --branch`\n```text\n");
        out.push_str(&s);
        out.push_str("\n```\n\n");
    }
    if let Some(d) = diff {
        out.push_str("### `git diff --stat`\n```text\n");
        out.push_str(&d);
        out.push_str("\n```\n");
    }
    Some(out)
}

fn truncate_single_line(s: &str, max_chars: usize) -> String {
    let single: String = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let count = single.chars().count();
    if count <= max_chars {
        single
    } else {
        let taken: String = single.chars().take(max_chars).collect();
        format!("{}...", taken)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_compact_handoff_file(
    paths: &AppPaths,
    working_dir: Option<&str>,
    short_id: &str,
    title: &str,
    source_kind: &str,
    messages: &[CanonicalMessage],
    recent_limit: usize,
    follow_up_prompt: Option<&str>,
) -> Result<(PathBuf, String), String> {
    create_handoff_markdown_file(
        paths,
        working_dir,
        short_id,
        title,
        source_kind,
        messages,
        recent_limit,
        follow_up_prompt,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_handoff_markdown_file(
    paths: &AppPaths,
    working_dir: Option<&str>,
    short_id: &str,
    title: &str,
    source_kind: &str,
    messages: &[CanonicalMessage],
    recent_limit: usize,
    follow_up_prompt: Option<&str>,
) -> Result<(PathBuf, String), String> {
    let (dest_path, prompt_ref) = if let Some(dir) = working_dir {
        let dir_path = Path::new(dir);
        let vault_dir = dir_path.join(".context-vault");
        fs::create_dir_all(&vault_dir)
            .map_err(|e| format!("创建交接目录失败 {}: {}", vault_dir.display(), e))?;
        ensure_git_info_exclude(dir_path);
        let rel = format!(".context-vault/handoff-{}.md", short_id);
        (vault_dir.join(format!("handoff-{}.md", short_id)), rel)
    } else {
        let handoff_dir = paths.data.join("handoffs");
        fs::create_dir_all(&handoff_dir)
            .map_err(|e| format!("创建交接目录失败 {}: {}", handoff_dir.display(), e))?;
        let full = handoff_dir.join(format!("handoff-{}.md", short_id));
        let fwd = full.to_string_lossy().replace('\\', "/");
        (full, fwd)
    };

    let source_label = match source_kind {
        "codex_main" => "Codex",
        "claude" => "Claude",
        "antigravity" => "Antigravity",
        other => other,
    };

    let mut md = String::new();
    md.push_str(&format!("# Handoff Context: {}\n\n", title));
    md.push_str(&format!("- **原会话来源**: {}\n", source_label));
    md.push_str(&format!(
        "- **生成时间**: {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    if let Some(dir) = working_dir {
        md.push_str(&format!("- **工作目录**: `{}`\n", dir));
    }
    md.push_str(&format!("- **总消息轮数**: {} 条有效消息\n\n", messages.len()));

    if let Some(dir) = working_dir {
        if let Some(git_ctx) = gather_git_context(Path::new(dir)) {
            md.push_str("## 当前工作区变更状态\n\n");
            md.push_str(&git_ctx);
            md.push('\n');
        }
    }

    let is_full = recent_limit == 0 || recent_limit >= messages.len();
    let recent_slice = if is_full {
        &messages[..]
    } else {
        let split = messages.len() - recent_limit;
        let earlier = &messages[..split];
        let earlier_prompts: Vec<String> = earlier
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .take(15)
            .map(|m| truncate_single_line(&m.content, 140))
            .collect();
        if !earlier_prompts.is_empty() {
            md.push_str(&format!(
                "## 前序话题脉络（已压缩前 {} 条消息）\n\n",
                earlier.len()
            ));
            for (idx, p) in earlier_prompts.iter().enumerate() {
                md.push_str(&format!("{}. {}\n", idx + 1, p));
            }
            md.push('\n');
        }
        &messages[split..]
    };

    let section_heading = if is_full && recent_limit == 0 {
        format!("## 完整前序对话原文（共 {} 条）\n\n", recent_slice.len())
    } else {
        format!("## 最近对话原文（共 {} 条）\n\n", recent_slice.len())
    };
    md.push_str(&section_heading);
    for (idx, msg) in recent_slice.iter().enumerate() {
        let role_title = if msg.role == MessageRole::User {
            "User"
        } else {
            "Assistant"
        };
        md.push_str(&format!("### {}. {}\n\n", idx + 1, role_title));
        md.push_str(msg.content.trim());
        md.push_str("\n\n");
    }

    md.push_str("## 下一步继续任务\n\n");
    let next_task = follow_up_prompt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("请基于上述交接上下文与工作区现状，直接延续最后讨论的目标继续推进或实现。");
    md.push_str(next_task);
    md.push('\n');

    write_atomic(&dest_path, md.as_bytes())?;
    Ok((dest_path, prompt_ref))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{Database, ImportedMessagePayload};
    use crate::services::AntigravityPublisher;

    struct TempWorkspace(PathBuf);

    impl TempWorkspace {
        fn new(prefix: &str) -> Self {
            let p = std::env::temp_dir().join(format!("cv-{}-{}", prefix, Uuid::new_v4()));
            fs::create_dir_all(&p).expect("create temp workspace");
            Self(p)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn make_messages() -> Vec<CanonicalMessage> {
        let now = Utc::now();
        vec![
            CanonicalMessage {
                id: "m1".to_string(),
                conversation_id: "c1".to_string(),
                parent_message_id: None,
                sequence_no: 1,
                role: MessageRole::User,
                content: "请重构登录模块并增加单元测试。".to_string(),
                status: crate::domain::MessageStatus::Complete,
                provider_id: None,
                model: None,
                remote_response_id: None,
                source_type: "claude_code".to_string(),
                source_external_id: None,
                created_at: now,
                updated_at: now,
            },
            CanonicalMessage {
                id: "m2".to_string(),
                conversation_id: "c1".to_string(),
                parent_message_id: Some("m1".to_string()),
                sequence_no: 2,
                role: MessageRole::Assistant,
                content: "已完成 auth.ts 提取，接下来只差集成测试。".to_string(),
                status: crate::domain::MessageStatus::Complete,
                provider_id: None,
                model: Some("claude-sonnet-4".to_string()),
                remote_response_id: None,
                source_type: "claude_code".to_string(),
                source_external_id: None,
                created_at: now,
                updated_at: now,
            },
        ]
    }

    #[test]
    fn strip_leading_cli_bin_removes_redundant_command_name() {
        assert_eq!(
            strip_leading_cli_bin(Some("codex --yolo")),
            Some("--yolo".to_string())
        );
        assert_eq!(
            strip_leading_cli_bin(Some("--dangerously-skip-permissions")),
            Some("--dangerously-skip-permissions".to_string())
        );
        assert_eq!(strip_leading_cli_bin(Some("claude")), None);
    }

    #[test]
    fn clean_working_dir_path_normalizes_slashes_and_drive_letter() {
        #[cfg(windows)]
        {
            assert_eq!(
                clean_working_dir_path("d:/repos/my-app/"),
                r"D:\repos\my-app"
            );
            assert_eq!(clean_working_dir_path("d:/"), r"D:\");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(clean_working_dir_path("/tmp/my-app/"), "/tmp/my-app");
            assert_eq!(clean_working_dir_path("/"), "/");
        }
    }

    #[test]
    fn inject_codex_session_preserves_existing_threads_and_writes_parseable_rollout() {
        let tmp = TempWorkspace::new("handoff-codex");
        let codex_home = tmp.path().join(".codex");
        let backups = tmp.path().join("backups");
        let project = tmp.path().join("my-repo");
        fs::create_dir_all(&codex_home).unwrap();
        fs::create_dir_all(&backups).unwrap();
        fs::create_dir_all(codex_home.join("sqlite")).unwrap();
        fs::create_dir_all(&project).unwrap();

        // Create existing state_5.sqlite + catalog + global state with an existing thread
        let state_conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        state_conn
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL, source TEXT NOT NULL, model_provider TEXT NOT NULL,
                    cwd TEXT NOT NULL, title TEXT NOT NULL, sandbox_policy TEXT NOT NULL,
                    approval_mode TEXT NOT NULL, custom_required_col TEXT NOT NULL
                );
                INSERT INTO threads VALUES ('existing-1', 'C:/old.jsonl', 1, 1, 'vscode', 'openai', 'C:/old', 'Old', '{}', 'never', 'x');",
            )
            .unwrap();
        drop(state_conn);

        let cat_conn = Connection::open(codex_home.join("sqlite/codex.db")).unwrap();
        cat_conn
            .execute_batch(
                "CREATE TABLE local_thread_catalog (
                    host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at REAL,
                    source_updated_at REAL, cwd TEXT, source_kind TEXT, model_provider TEXT,
                    observation_sequence INTEGER, missing_candidate INTEGER, thread_source TEXT,
                    source_recency_at REAL, pending_observed_title INTEGER, PRIMARY KEY (host_id, thread_id)
                );
                INSERT INTO local_thread_catalog VALUES ('local', 'existing-1', 'Old', 1.0, 1.0, 'C:/old', 'vscode', 'openai', 5, 0, 'user', 1.0, 0);",
            )
            .unwrap();
        drop(cat_conn);

        fs::write(
            codex_home.join(".codex-global-state.json"),
            serde_json::json!({
                "local-projects": {
                    "proj-1": { "rootPaths": [project.to_string_lossy()] }
                },
                "thread-project-assignments": {}
            })
            .to_string(),
        )
        .unwrap();

        let new_id = Uuid::new_v4().to_string();
        let mut warnings = Vec::new();
        let rollout_path = inject_codex_session(
            &codex_home,
            &backups,
            &new_id,
            "跨工具迁移测试",
            Some(&project.to_string_lossy()),
            &make_messages(),
            &mut warnings,
        )
        .expect("inject codex");
        assert!(warnings.is_empty(), "{:?}", warnings);
        assert!(rollout_path.is_file());

        // Verify CodexRecoveryService parses the synthesized rollout
        let parsed = CodexRecoveryService::parse_rollout(&rollout_path);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].role, "user");
        assert_eq!(parsed[1].role, "assistant");

        // Verify existing thread was NOT hidden or deleted in state_5 or catalog
        let check_state = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        let old_provider: String = check_state
            .query_row("SELECT model_provider FROM threads WHERE id = 'existing-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(old_provider, "openai");
        let new_title: String = check_state
            .query_row("SELECT title FROM threads WHERE id = ?1", [&new_id], |r| r.get(0))
            .unwrap();
        assert_eq!(new_title, "跨工具迁移测试");

        let check_cat = Connection::open(codex_home.join("sqlite/codex.db")).unwrap();
        let count_cat: i64 = check_cat
            .query_row("SELECT COUNT(*) FROM local_thread_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_cat, 2);

        let gs: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(codex_home.join(".codex-global-state.json")).unwrap())
                .unwrap();
        assert_eq!(
            gs["thread-project-assignments"][&new_id]["projectId"].as_str(),
            Some("proj-1")
        );
    }

    #[test]
    fn compact_handoff_writes_markdown_updates_git_exclude_and_builds_safe_cli_cmd() {
        let tmp = TempWorkspace::new("handoff-compact");
        let repo_dir = tmp.path().join("workspace");
        let data_dir = tmp.path().join("data");
        let backups = tmp.path().join("backups");
        fs::create_dir_all(repo_dir.join(".git/info")).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&backups).unwrap();

        let db = Database::new(&data_dir.join("vault.db"), &backups).unwrap();
        let repo = Repository::new(&db);
        let (conv_id, _, _, _) = repo
            .sync_import_conversation(
                "优化会话切号链路",
                CodexRecoveryService::SOURCE_TYPE,
                "cx-thread-1",
                &[
                    ImportedMessagePayload {
                        role: "user".to_string(),
                        content: "第一轮需求描述".to_string(),
                        external_id: "e1".to_string(),
                        parent_external_id: None,
                        created_at: None,
                    },
                    ImportedMessagePayload {
                        role: "assistant".to_string(),
                        content: "第一轮架构回复".to_string(),
                        external_id: "e2".to_string(),
                        parent_external_id: Some("e1".to_string()),
                        created_at: None,
                    },
                    ImportedMessagePayload {
                        role: "user".to_string(),
                        content: "请接着实现前端弹窗".to_string(),
                        external_id: "e3".to_string(),
                        parent_external_id: Some("e2".to_string()),
                        created_at: None,
                    },
                ],
                Some("e3"),
                Some(&repo_dir.to_string_lossy()),
                "codex_main",
            )
            .unwrap();

        let paths = AppPaths {
            root: tmp.path().to_path_buf(),
            data: data_dir,
            database: tmp.path().join("data/vault.db"),
            backups,
            logs: tmp.path().join("logs"),
            import_reports: tmp.path().join("reports"),
        };

        let gemini_home = tmp.path().join(".gemini");
        let homes = HandoffTargetHomes {
            codex_home: tmp.path().join(".codex"),
            claude_home: tmp.path().join(".claude"),
            gemini_home: gemini_home.clone(),
            claude_data_dir: None,
            antigravity_app_storage: None,
        };

        let result = execute_handoff_with_homes(
            &paths,
            &db,
            HandoffRequest {
                conversation_id: conv_id,
                target_client: AccountClient::Antigravity,
                target_account_id: None,
                mode: HandoffMode::Compact,
                tool_id: "antigravity_cli".to_string(),
                folder_path: None,
                terminal: Some("wt".to_string()),
                extra_cli_args: Some("agy --model gemini-3-pro".to_string()),
                recent_message_limit: Some(2),
                follow_up_prompt: Some("补充测试并用中文回复".to_string()),
                sync_latest_first: false,
                prepare_only: true,
                restart_desktop_if_running: None,
            },
            &homes,
        )
        .expect("execute compact handoff");

        assert!(!result.launched);
        assert!(!result.workspace_created);
        let ag_pid = result.workspace_project_id.clone().expect("ag project created");
        assert!(gemini_home.join("config/projects").join(format!("{}.json", ag_pid)).is_file());

        let handoff_path = PathBuf::from(result.handoff_file_path.as_ref().unwrap());
        assert!(handoff_path.is_file());
        let md = fs::read_to_string(&handoff_path).unwrap();
        assert!(md.contains("[续聊] 优化会话切号链路"));
        assert!(md.contains("前序话题脉络"));
        assert!(md.contains("最近对话原文（共 2 条）"));
        assert!(md.contains("补充测试并用中文回复"));

        let exclude = fs::read_to_string(repo_dir.join(".git/info/exclude")).unwrap();
        assert!(exclude.contains(".context-vault/"));
        assert!(result.cli_command.starts_with(&format!(
            "agy --project {} --model gemini-3-pro -i \"请先阅读 .context-vault/handoff-",
            ag_pid
        )));
        assert!(!result.clipboard_prompt.contains('\n'));
    }

    #[test]
    fn resolves_creates_and_idempotently_reuses_workspace_and_client_projects() {
        let tmp = TempWorkspace::new("handoff-ws-reuse");
        let new_work_dir = tmp.path().join("brand-new-workspace");
        assert!(!new_work_dir.exists());

        // 1. 不存在的目录会自动创建；再次调用复用同一目录且不再创建
        let (dir_1, created_1) = resolve_or_create_working_dir(
            Some(&new_work_dir.to_string_lossy()),
            None,
            "新项目对话",
        )
        .expect("create missing working dir");
        assert!(created_1);
        assert!(Path::new(&dir_1).is_dir());

        let (dir_2, created_2) = resolve_or_create_working_dir(
            Some(&new_work_dir.to_string_lossy()),
            None,
            "新项目对话",
        )
        .expect("reuse existing working dir");
        assert!(!created_2);
        assert_eq!(dir_1, dir_2);

        // 2. Antigravity 工作区 + app_storage.json：首次创建，再次复用同 ID
        let gemini_home = tmp.path().join(".gemini");
        let app_storage = tmp.path().join("app_storage.json");
        fs::write(&app_storage, r#"{"projectsOrder":"[]"}"#).unwrap();

        let ag_pid_1 =
            ensure_antigravity_workspace_project(&gemini_home, Some(&app_storage), &dir_1)
                .expect("create ag project");
        let ag_pid_2 =
            ensure_antigravity_workspace_project(&gemini_home, Some(&app_storage), &dir_1)
                .expect("reuse ag project");
        assert_eq!(ag_pid_1, ag_pid_2);
        let proj_files: Vec<_> = fs::read_dir(gemini_home.join("config/projects"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(proj_files.len(), 1);
        let storage_val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&app_storage).unwrap()).unwrap();
        assert_eq!(
            storage_val["new-convo-last-selected-project"].as_str(),
            Some(ag_pid_1.as_str())
        );

        // 3. Claude ccd-session-presets.json：首次写入 preset，再次复用而不重复添加
        let claude_home = tmp.path().join(".claude");
        let claude_data = tmp.path().join("Claude-Data");
        let presets_path = claude_data.join("claude_testpkg/ccd-session-presets.json");
        fs::create_dir_all(presets_path.parent().unwrap()).unwrap();
        fs::write(
            &presets_path,
            serde_json::json!({
                "byAccount": {
                    "acct-1": { "presets": [] }
                }
            })
            .to_string(),
        )
        .unwrap();

        let preset_1 = ensure_claude_workspace_preset_in_dir(
            &claude_home,
            Some(&claude_data),
            &dir_1,
        )
        .expect("create claude preset")
        .expect("preset id");
        let preset_2 = ensure_claude_workspace_preset_in_dir(
            &claude_home,
            Some(&claude_data),
            &dir_1,
        )
        .expect("reuse claude preset")
        .expect("preset id");
        assert_eq!(preset_1, preset_2);
        let presets_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&presets_path).unwrap()).unwrap();
        assert_eq!(presets_json["byAccount"]["acct-1"]["presets"].as_array().unwrap().len(), 1);

        // 4. Codex state_5 (projects + project_roots + threads) + thread_history_1 + .codex-global-state.json
        let codex_home = tmp.path().join(".codex");
        let backups = tmp.path().join("backups");
        fs::create_dir_all(&codex_home).unwrap();
        fs::create_dir_all(&backups).unwrap();

        let state_conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        state_conn
            .execute_batch(
                "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, metadata TEXT NOT NULL DEFAULT '{}', position INTEGER NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
                 CREATE TABLE project_roots (project_id TEXT NOT NULL, position INTEGER NOT NULL, path TEXT NOT NULL, PRIMARY KEY (project_id, position));
                 CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, source TEXT NOT NULL, model_provider TEXT NOT NULL, cwd TEXT NOT NULL, title TEXT NOT NULL, sandbox_policy TEXT NOT NULL, approval_mode TEXT NOT NULL, project_id TEXT, history_mode TEXT NOT NULL DEFAULT 'legacy');",
            )
            .unwrap();
        drop(state_conn);

        let hist_conn = Connection::open(codex_home.join("thread_history_1.sqlite")).unwrap();
        hist_conn
            .execute_batch(
                "CREATE TABLE thread_turns (thread_id TEXT NOT NULL, turn_id TEXT NOT NULL, rollout_ordinal INTEGER NOT NULL, status TEXT NOT NULL, error_json TEXT, started_at INTEGER, completed_at INTEGER, duration_ms INTEGER, first_user_item_id TEXT, final_agent_item_id TEXT, rollout_byte_offset INTEGER, rollout_end_ordinal INTEGER, rollout_end_byte_offset INTEGER, root_turn_id TEXT, PRIMARY KEY (thread_id, turn_id));
                 CREATE TABLE thread_items (thread_id TEXT NOT NULL, turn_id TEXT NOT NULL, item_id TEXT NOT NULL, rollout_ordinal INTEGER NOT NULL, created_at_ms INTEGER NOT NULL, item_json TEXT NOT NULL, item_type TEXT NOT NULL DEFAULT '', updated_at_ordinal INTEGER NOT NULL DEFAULT 0, started_at_ms INTEGER, completed_at_ms INTEGER, PRIMARY KEY (thread_id, turn_id, item_id));
                 CREATE TABLE thread_history_projection_state (thread_id TEXT PRIMARY KEY, next_rollout_byte_offset INTEGER NOT NULL, next_rollout_ordinal INTEGER NOT NULL);",
            )
            .unwrap();
        drop(hist_conn);

        fs::write(codex_home.join(".codex-global-state.json"), "{}").unwrap();

        let mut w = Vec::new();
        let ids_1 = ensure_codex_workspace_project(&codex_home, &backups, &dir_1, &mut w);
        let ids_2 = ensure_codex_workspace_project(&codex_home, &backups, &dir_1, &mut w);
        assert!(w.is_empty(), "{:?}", w);
        assert_eq!(ids_1.state_project_id, ids_2.state_project_id);
        assert_eq!(ids_1.legacy_project_id, ids_2.legacy_project_id);

        let tid = Uuid::new_v4().to_string();
        inject_codex_session_with_projects(
            &codex_home,
            &backups,
            &tid,
            "[续聊] 聚焦项目",
            Some(&dir_1),
            &ids_1,
            &make_messages(),
            &mut w,
        )
        .expect("inject codex paginated");
        assert!(w.is_empty(), "{:?}", w);

        let sc = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        let (hmode, proj_id): (String, Option<String>) = sc
            .query_row(
                "SELECT history_mode, project_id FROM threads WHERE id = ?1",
                [&tid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(hmode, "paginated");
        assert_eq!(proj_id, ids_1.state_project_id);

        let hc = Connection::open(codex_home.join("thread_history_1.sqlite")).unwrap();
        let item_count: i64 = hc
            .query_row(
                "SELECT COUNT(*) FROM thread_items WHERE thread_id = ?1",
                [&tid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(item_count, 2);
    }

    #[test]
    fn desktop_handoff_builds_deep_links_for_codex_and_claude_and_full_doc_for_antigravity() {
        let tmp = TempWorkspace::new("handoff-deeplinks");
        let data_dir = tmp.path().join("data");
        let backups = tmp.path().join("backups");
        let target_dir = tmp.path().join("auto-created-proj");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&backups).unwrap();

        let db = Database::new(&data_dir.join("vault.db"), &backups).unwrap();
        let repo = Repository::new(&db);
        let (conv_id, _, _, _) = repo
            .sync_import_conversation(
                "调试桌面直达链接",
                ClaudeRecoveryService::SOURCE_TYPE,
                "cl-sess-1",
                &[
                    ImportedMessagePayload {
                        role: "user".to_string(),
                        content: "第一句".to_string(),
                        external_id: "m1".to_string(),
                        parent_external_id: None,
                        created_at: None,
                    },
                    ImportedMessagePayload {
                        role: "assistant".to_string(),
                        content: "第一答".to_string(),
                        external_id: "m2".to_string(),
                        parent_external_id: Some("m1".to_string()),
                        created_at: None,
                    },
                ],
                Some("m2"),
                None,
                "claude",
            )
            .unwrap();

        let paths = AppPaths {
            root: tmp.path().to_path_buf(),
            data: data_dir,
            database: tmp.path().join("data/vault.db"),
            backups,
            logs: tmp.path().join("logs"),
            import_reports: tmp.path().join("reports"),
        };
        let homes = HandoffTargetHomes {
            codex_home: tmp.path().join(".codex"),
            claude_home: tmp.path().join(".claude"),
            gemini_home: tmp.path().join(".gemini"),
            claude_data_dir: None,
            antigravity_app_storage: None,
        };

        // Codex App Mode A -> codex://threads/<session_id> & creates missing workspace dir
        let res_codex_a = execute_handoff_with_homes(
            &paths,
            &db,
            HandoffRequest {
                conversation_id: conv_id.clone(),
                target_client: AccountClient::Codex,
                target_account_id: None,
                mode: HandoffMode::Native,
                tool_id: "codex_app".to_string(),
                folder_path: Some(target_dir.to_string_lossy().to_string()),
                terminal: None,
                extra_cli_args: None,
                recent_message_limit: None,
                follow_up_prompt: None,
                sync_latest_first: false,
                prepare_only: true,
                restart_desktop_if_running: None,
            },
            &homes,
        )
        .expect("codex app mode A");
        assert!(res_codex_a.workspace_created);
        assert!(target_dir.is_dir());
        assert!(res_codex_a
            .deep_link_uri
            .as_deref()
            .unwrap()
            .starts_with("codex://threads/"));
        // State_5 should NOT have been created from scratch without migrations
        assert!(!homes.codex_home.join("state_5.sqlite").exists());

        // Claude App Mode A -> claude://resume?session=<uuid>, reuses existing workspace dir
        let res_claude_a = execute_handoff_with_homes(
            &paths,
            &db,
            HandoffRequest {
                conversation_id: conv_id.clone(),
                target_client: AccountClient::Claude,
                target_account_id: None,
                mode: HandoffMode::Native,
                tool_id: "claude_app".to_string(),
                folder_path: Some(target_dir.to_string_lossy().to_string()),
                terminal: None,
                extra_cli_args: None,
                recent_message_limit: None,
                follow_up_prompt: None,
                sync_latest_first: false,
                prepare_only: true,
                restart_desktop_if_running: None,
            },
            &homes,
        )
        .expect("claude app mode A");
        assert!(!res_claude_a.workspace_created);
        assert_eq!(
            res_claude_a.deep_link_uri.as_deref(),
            Some(format!("claude://resume?session={}", res_claude_a.target_session_id.unwrap()).as_str())
        );

        // Claude App Mode B -> claude://code/new?folder=...&prompt=...
        let res_claude_b = execute_handoff_with_homes(
            &paths,
            &db,
            HandoffRequest {
                conversation_id: conv_id.clone(),
                target_client: AccountClient::Claude,
                target_account_id: None,
                mode: HandoffMode::Compact,
                tool_id: "claude_app".to_string(),
                folder_path: Some(target_dir.to_string_lossy().to_string()),
                terminal: None,
                extra_cli_args: None,
                recent_message_limit: Some(5),
                follow_up_prompt: None,
                sync_latest_first: false,
                prepare_only: true,
                restart_desktop_if_running: None,
            },
            &homes,
        )
        .expect("claude app mode B");
        let claude_link = res_claude_b.deep_link_uri.as_deref().unwrap();
        assert!(claude_link.starts_with("claude://code/new?folder="));
        assert!(claude_link.contains("&prompt="));

        // Antigravity Mode A -> full verbatim doc + agy --project <id> -i ...
        let res_ag_a = execute_handoff_with_homes(
            &paths,
            &db,
            HandoffRequest {
                conversation_id: conv_id,
                target_client: AccountClient::Antigravity,
                target_account_id: None,
                mode: HandoffMode::Native,
                tool_id: "antigravity_cli".to_string(),
                folder_path: Some(target_dir.to_string_lossy().to_string()),
                terminal: None,
                extra_cli_args: None,
                recent_message_limit: None,
                follow_up_prompt: None,
                sync_latest_first: false,
                prepare_only: true,
                restart_desktop_if_running: None,
            },
            &homes,
        )
        .expect("antigravity mode A");
        let ag_doc = fs::read_to_string(res_ag_a.handoff_file_path.unwrap()).unwrap();
        assert!(ag_doc.contains("完整前序对话原文"));
        assert!(res_ag_a.cli_command.starts_with(&format!(
            "agy --project {} -i ",
            res_ag_a.workspace_project_id.unwrap()
        )));
    }

    #[test]
    fn native_handoff_injects_into_claude_and_antigravity_and_survives_rescan() {
        let tmp = TempWorkspace::new("handoff-native-multi");
        let claude_home = tmp.path().join(".claude");
        let ag_home = tmp.path().join(".gemini/antigravity");
        let data_dir = tmp.path().join("data");
        let backups = tmp.path().join("backups");
        let work_dir = tmp.path().join("app-src");
        fs::create_dir_all(&work_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&backups).unwrap();

        let db = Database::new(&data_dir.join("vault.db"), &backups).unwrap();
        let msgs = make_messages();

        let claude_id = Uuid::new_v4().to_string();
        let claude_file = inject_claude_session(
            &claude_home,
            &backups,
            &claude_id,
            "Codex -> Claude 接力",
            Some(&work_dir.to_string_lossy()),
            &msgs,
        )
        .expect("inject claude");
        assert!(claude_file.is_file());
        let parsed_claude = ClaudeRecoveryService::parse_session_file(&claude_file, &claude_id);
        assert_eq!(parsed_claude.len(), 2);

        let ag_id = Uuid::new_v4().to_string();
        let ag_pub = AntigravityPublisher::new(&db, Some(&ag_home), &backups);
        ag_pub
            .inject_handoff_conversation(
                &ag_id,
                "Claude -> Antigravity 接力",
                Some(&work_dir.to_string_lossy()),
                &msgs,
            )
            .expect("inject antigravity");
        let ag_rec = AntigravityRecoveryService::new(&db, Some(&ag_home));
        let scan = ag_rec.scan();
        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.candidates[0].conversation_id, ag_id);
        assert_eq!(scan.candidates[0].title, "Claude -> Antigravity 接力");
    }
}
