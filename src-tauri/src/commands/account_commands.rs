//! 账号管理命令。每个命令都带 `client`，三个客户端（Codex、Claude、Antigravity）的账号分开保存、分开切换。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use super::run_blocking;
use crate::services::{
    account_store, AccountClient, AccountsState, AntigravityAccountManager, AntigravityClientController, CcSwitchState,
    ClaudeAccountManager, ClaudeClientController, CodexAccountManager, CodexClientController, CodexDeviceAuthStart,
    CodexProviderInput, ProviderInput, QuickLaunchService,
};
use crate::AppState;

/// 正在进行的官方登录进程，供取消使用。
static LOGIN_PID: AtomicU32 = AtomicU32::new(0);
/// 浏览器 OAuth / Device Code 取消信号。
static LOGIN_CANCELLED: AtomicBool = AtomicBool::new(false);
/// 用户在 UI 中手动粘贴的 OAuth 回调链接或 `code#state`。
static MANUAL_CALLBACK: Mutex<Option<String>> = Mutex::new(None);
/// 等待浏览器登录的最长时间。
const LOGIN_TIMEOUT_SECS: u64 = 600;

fn take_manual_callback() -> Option<String> {
    MANUAL_CALLBACK.lock().ok()?.take()
}

fn clear_manual_callback() {
    if let Ok(mut guard) = MANUAL_CALLBACK.lock() {
        *guard = None;
    }
}

enum Manager {
    Codex(CodexAccountManager),
    Claude(ClaudeAccountManager),
    Antigravity(AntigravityAccountManager),
}

impl Manager {
    fn of(state: &AppState, client: AccountClient) -> Self {
        let p = &state.paths;
        match client {
            AccountClient::Codex => Manager::Codex(CodexAccountManager::new(&p.codex_home(), &p.data, &p.backups)),
            AccountClient::Claude => Manager::Claude(ClaudeAccountManager::new(&p.claude_home(), &p.claude_json(), &p.data, &p.backups)),
            AccountClient::Antigravity => Manager::Antigravity(AntigravityAccountManager::new_live(
                &crate::services::antigravity_accounts::default_state_db(),
                &p.data,
                &p.backups,
                Some(p.gemini_home()),
            )),
        }
    }

    fn state(&self) -> Result<AccountsState, String> {
        let mut st = match self {
            Manager::Codex(m) => m.state()?,
            Manager::Claude(m) => m.state()?,
            Manager::Antigravity(m) => m.state()?,
        };
        let cockpit_dirs = account_store::cockpit_data_dirs();
        st.cockpit_import_count = match self {
            Manager::Codex(m) => m.count_cockpit_tools_in(&cockpit_dirs),
            Manager::Claude(m) => m.count_cockpit_tools_in(&cockpit_dirs),
            Manager::Antigravity(m) => m.count_cockpit_tools_in(&cockpit_dirs),
        };
        st.cc_switch_available = match self {
            Manager::Codex(_) | Manager::Claude(_) => CcSwitchState::detect().database.is_some(),
            Manager::Antigravity(_) => false,
        };
        Ok(st)
    }

    fn save_current(&self) -> Result<String, String> {
        match self {
            Manager::Codex(m) => m.save_current(),
            Manager::Claude(m) => m.save_current(),
            Manager::Antigravity(m) => m.save_current(),
        }
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        match self {
            Manager::Codex(m) => m.delete(id),
            Manager::Claude(m) => m.delete(id),
            Manager::Antigravity(m) => m.delete(id),
        }
    }

    fn refresh_quota(&self, id: &str) -> Result<(), String> {
        match self {
            Manager::Codex(m) => m.refresh_quota(id),
            Manager::Claude(m) => m.refresh_quota(id),
            Manager::Antigravity(m) => m.refresh_quota(id),
        }
    }

    fn update_meta(&self, id: &str, tags: Option<Vec<String>>, note: Option<Option<String>>) -> Result<(), String> {
        match self {
            Manager::Codex(m) => m.update_meta(id, tags, note),
            Manager::Claude(m) => m.update_meta(id, tags, note),
            Manager::Antigravity(m) => m.update_meta(id, tags, note),
        }
    }

    fn batch_update_tags(&self, ids: &[String], add: &[String], remove: &[String]) -> Result<usize, String> {
        match self {
            Manager::Codex(m) => m.batch_update_tags(ids, add, remove),
            Manager::Claude(m) => m.batch_update_tags(ids, add, remove),
            Manager::Antigravity(m) => m.batch_update_tags(ids, add, remove),
        }
    }

    fn batch_delete(&self, ids: &[String]) -> Result<usize, String> {
        match self {
            Manager::Codex(m) => m.batch_delete(ids),
            Manager::Claude(m) => m.batch_delete(ids),
            Manager::Antigravity(m) => m.batch_delete(ids),
        }
    }

    fn export_json(&self, ids: Option<&[String]>) -> Result<String, String> {
        match self {
            Manager::Codex(m) => m.export_json(ids),
            Manager::Claude(m) => m.export_json(ids),
            Manager::Antigravity(m) => m.export_json(ids),
        }
    }
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>, client: AccountClient) -> Result<AccountsState, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.state()).await
}

#[tauri::command]
pub async fn save_current_account(state: State<'_, AppState>, client: AccountClient) -> Result<String, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let id = m.save_current()?;
        let _ = m.refresh_quota(&id);
        Ok(id)
    })
    .await
}

/// 从本地文件导入账号（支持 Codex `auth.json`、Claude `.credentials.json`、三端账号 JSON / Token 文本与 Cockpit 导出文件）。
#[tauri::command]
pub async fn import_account_file(state: State<'_, AppState>, client: AccountClient, path: String) -> Result<TokenImportResult, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let norm = crate::util::normalize_windows_path(&path);
        let text = std::fs::read_to_string(&norm).map_err(|e| format!("读取 {} 失败：{}", norm, e))?;
        import_tokens_for(&m, &text)
    })
    .await
}

/// 手动提交浏览器 OAuth 回调 URL 或 `code#state`，供远程或端口受阻时兜底。
#[tauri::command]
pub async fn submit_account_login_callback(callback_url: String) -> Result<(), String> {
    let trimmed = callback_url.trim().to_string();
    if trimmed.is_empty() {
        return Err("请粘贴浏览器回调链接或授权码".into());
    }
    let mut guard = MANUAL_CALLBACK.lock().map_err(|_| "内部状态锁定失败".to_string())?;
    *guard = Some(trimmed);
    Ok(())
}

/// 在浏览器里登录新账号并保存，当前登录不受影响：
/// - Antigravity：浏览器 Google PKCE OAuth（支持手动回调兜底）；
/// - Codex：默认 `oauth` 浏览器 PKCE（1455），或 `mode = "cli"` 启动官方 `codex login`；
/// - Claude：默认 `cli` 官方 `claude auth login`，或 `mode = "pkce"` 浏览器 OAuth。
#[tauri::command]
pub async fn login_account(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    client: AccountClient,
    mode: Option<String>,
) -> Result<String, String> {
    let m = Manager::of(&state, client);
    let dir = state.paths.data.join("accounts").join(format!("login-{}", uuid::Uuid::new_v4()));
    LOGIN_CANCELLED.store(false, Ordering::SeqCst);
    clear_manual_callback();
    run_blocking(move || {
        let id = match &m {
            Manager::Antigravity(ag) => ag.login_in_browser_with_manual(
                &|url| { let _ = app.emit("account-login-url", url); },
                &|| LOGIN_CANCELLED.load(Ordering::SeqCst),
                &take_manual_callback,
            )?,
            Manager::Codex(cx) if mode.as_deref() == Some("cli") => {
                std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建临时目录：{}", e))?;
                let res = run_login(codex_login_command(&dir), &dir.join("auth.json")).and_then(|_| cx.import_login_dir(&dir));
                let _ = std::fs::remove_dir_all(&dir);
                res?
            }
            Manager::Codex(cx) => cx.login_oauth_pkce(
                &|url| { let _ = app.emit("account-login-url", url); },
                &|| LOGIN_CANCELLED.load(Ordering::SeqCst),
                &take_manual_callback,
            )?,
            Manager::Claude(cl) if mode.as_deref() == Some("pkce") => cl.login_oauth_pkce(
                &|url| { let _ = app.emit("account-login-url", url); },
                &|| LOGIN_CANCELLED.load(Ordering::SeqCst),
                &take_manual_callback,
            )?,
            Manager::Claude(cl) => {
                std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建临时目录：{}", e))?;
                let res = run_login(claude_login_command(&dir), &dir.join(".credentials.json")).and_then(|_| cl.import_login_dir(&dir));
                let _ = std::fs::remove_dir_all(&dir);
                res?
            }
        };
        let _ = m.refresh_quota(&id);
        Ok(id)
    })
    .await
}

/// Codex OpenAI Device Code 登录第 1 步。
#[tauri::command]
pub async fn start_codex_device_login(state: State<'_, AppState>) -> Result<CodexDeviceAuthStart, String> {
    let p = &state.paths;
    let m = CodexAccountManager::new(&p.codex_home(), &p.data, &p.backups);
    run_blocking(move || m.start_device_login()).await
}

/// Codex OpenAI Device Code 登录第 2 步（轮询并保存账号）。
#[tauri::command]
pub async fn complete_codex_device_login(state: State<'_, AppState>, start: CodexDeviceAuthStart) -> Result<String, String> {
    let p = &state.paths;
    let m = CodexAccountManager::new(&p.codex_home(), &p.data, &p.backups);
    LOGIN_CANCELLED.store(false, Ordering::SeqCst);
    run_blocking(move || {
        let id = m.complete_device_login(&start, &|| LOGIN_CANCELLED.load(Ordering::SeqCst))?;
        let _ = m.refresh_quota(&id);
        Ok(id)
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct TokenImportResult {
    pub imported: usize,
    pub failures: Vec<String>,
}

fn import_tokens_for(m: &Manager, text: &str) -> Result<TokenImportResult, String> {
    let (ids, failures) = match m {
        Manager::Antigravity(ag) => {
            let r = ag.import_tokens(text)?;
            (r.ids, r.failures)
        }
        Manager::Codex(cx) => {
            let r = cx.import_tokens(text)?;
            (r.ids, r.failures)
        }
        Manager::Claude(cl) => {
            let r = cl.import_tokens(text)?;
            (r.ids, r.failures)
        }
    };
    for id in &ids {
        let _ = m.refresh_quota(id);
    }
    Ok(TokenImportResult {
        imported: ids.len(),
        failures,
    })
}

/// 批量导入 token 或账号 JSON（支持 Antigravity `1//…` / Codex `rt_…` / `auth.json` / Claude token / 导出 JSON），导入后刷新一次额度。
#[tauri::command]
pub async fn import_account_tokens(state: State<'_, AppState>, client: AccountClient, text: String) -> Result<TokenImportResult, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || import_tokens_for(&m, &text)).await
}

#[tauri::command]
pub async fn cancel_account_login() -> Result<(), String> {
    LOGIN_CANCELLED.store(true, Ordering::SeqCst);
    clear_manual_callback();
    let pid = LOGIN_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        run_blocking(move || {
            let _ = crate::services::process_util::kill_tree(pid);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

/// 切换账号，返回是否重启了客户端。
/// - Codex：`restart` 且桌面版在运行时，先退出（避免它退出时把旧 token 写回），切换后重新打开。
/// - Antigravity：若存在旧版 `state.vscdb` 且在运行须先关闭或 `restart`；纯 2.0 凭据模式支持直接热切换（勾选重启时仍可重启桌面端）。
/// - Claude：新开的 Claude Code 会话自动使用新账号，不重启任何程序。
#[tauri::command]
pub async fn switch_account(state: State<'_, AppState>, client: AccountClient, id: String, restart: bool) -> Result<bool, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || match m {
        Manager::Codex(m) => {
            let controller = CodexClientController::new();
            let should_restart = restart && controller.is_running();
            if should_restart {
                controller.stop(15.0)?;
            }
            let result = m.switch(&id);
            if should_restart {
                controller.start(15.0)?;
            }
            result.map(|_| should_restart)
        }
        Manager::Claude(m) => m.switch(&id).map(|_| false),
        Manager::Antigravity(m) => {
            let has_legacy_db = crate::services::antigravity_accounts::default_state_db().is_file();
            let controller = AntigravityClientController::new();
            let running = controller.is_running();
            if has_legacy_db && running && !restart {
                return Err("Antigravity 正在运行，请先关闭它，或勾选“切换后重启”".into());
            }
            let should_restart = running && restart;
            if should_restart {
                controller.stop(15.0)?;
            }
            let result = m.switch(&id);
            if should_restart {
                controller.start(15.0)?;
            }
            result.map(|_| should_restart)
        }
    })
    .await
}

#[tauri::command]
pub async fn delete_account(state: State<'_, AppState>, client: AccountClient, id: String) -> Result<(), String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.delete(&id)).await
}

#[tauri::command]
pub async fn update_account_meta(
    state: State<'_, AppState>,
    client: AccountClient,
    id: String,
    tags: Option<Vec<String>>,
    note: Option<Option<String>>,
) -> Result<(), String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.update_meta(&id, tags, note)).await
}

#[tauri::command]
pub async fn batch_update_account_tags(
    state: State<'_, AppState>,
    client: AccountClient,
    ids: Vec<String>,
    add_tags: Vec<String>,
    remove_tags: Vec<String>,
) -> Result<usize, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.batch_update_tags(&ids, &add_tags, &remove_tags)).await
}

#[tauri::command]
pub async fn batch_delete_accounts(state: State<'_, AppState>, client: AccountClient, ids: Vec<String>) -> Result<usize, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.batch_delete(&ids)).await
}

#[tauri::command]
pub async fn refresh_account_quota(state: State<'_, AppState>, client: AccountClient, id: String) -> Result<(), String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.refresh_quota(&id)).await
}

#[derive(Serialize)]
pub struct QuotaRefreshFailure {
    pub id: String,
    pub error: String,
}

/// 依次刷新该客户端所有（或指定 `ids`）能查额度的账号，返回失败的账号。
#[tauri::command]
pub async fn refresh_all_account_quotas(
    state: State<'_, AppState>,
    client: AccountClient,
    ids: Option<Vec<String>>,
) -> Result<Vec<QuotaRefreshFailure>, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let target_ids: Vec<String> = m
            .state()?
            .accounts
            .into_iter()
            .filter(|a| a.has_quota && ids.as_ref().is_none_or(|filter| filter.iter().any(|x| x == &a.id)))
            .map(|a| a.id)
            .collect();
        Ok(target_ids
            .into_iter()
            .filter_map(|id| m.refresh_quota(&id).err().map(|error| QuotaRefreshFailure { id, error }))
            .collect())
    })
    .await
}

/// 新增或编辑 Claude 中转供应商。
#[tauri::command]
pub async fn upsert_claude_provider(state: State<'_, AppState>, input: ProviderInput) -> Result<String, String> {
    let p = &state.paths;
    let m = ClaudeAccountManager::new(&p.claude_home(), &p.claude_json(), &p.data, &p.backups);
    run_blocking(move || m.upsert_provider(input)).await
}

/// 新增或编辑 Codex 中转供应商。
#[tauri::command]
pub async fn upsert_codex_provider(state: State<'_, AppState>, input: CodexProviderInput) -> Result<String, String> {
    let p = &state.paths;
    let m = CodexAccountManager::new(&p.codex_home(), &p.data, &p.backups);
    run_blocking(move || m.upsert_provider(input)).await
}

#[derive(Debug, Serialize)]
pub struct ProviderTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub models: Vec<String>,
    pub message: String,
}

/// 测试第三方中转供应商 `/v1/models` 连通性与可用模型列表。
#[tauri::command]
pub async fn test_provider_connection(
    base_url: String,
    api_key: String,
    auth_field: Option<String>,
) -> Result<ProviderTestResult, String> {
    run_blocking(move || {
        let clean = base_url.trim().trim_end_matches('/').to_string();
        if !(clean.starts_with("https://") || clean.starts_with("http://")) {
            return Err("接口地址必须以 http:// 或 https:// 开头".into());
        }
        let key = api_key.trim().to_string();
        if key.is_empty() {
            return Err("请填写 API Key / Token".into());
        }
        let models_url = if clean.ends_with("/v1") {
            format!("{}/models", clean)
        } else {
            format!("{}/v1/models", clean)
        };

        let start = Instant::now();
        let prefer_x_api_key = auth_field.as_deref() == Some("ANTHROPIC_API_KEY");
        let send = |use_x_api_key: bool| {
            let bearer = format!("Bearer {}", key);
            if use_x_api_key {
                crate::services::win_http::request(
                    "GET",
                    &models_url,
                    &[
                        ("x-api-key", key.as_str()),
                        ("anthropic-version", "2023-06-01"),
                        ("Accept", "application/json"),
                    ],
                    None,
                )
            } else {
                crate::services::win_http::request(
                    "GET",
                    &models_url,
                    &[("Authorization", bearer.as_str()), ("Accept", "application/json")],
                    None,
                )
            }
        };

        let mut resp = match send(prefer_x_api_key) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ProviderTestResult {
                    ok: false,
                    latency_ms: start.elapsed().as_millis() as u64,
                    models: Vec::new(),
                    message: e,
                })
            }
        };
        if matches!(resp.status, 401 | 403) {
            if let Ok(fallback_resp) = send(!prefer_x_api_key) {
                if (200..300).contains(&fallback_resp.status) {
                    resp = fallback_resp;
                }
            }
        }
        let latency_ms = start.elapsed().as_millis() as u64;
        if !(200..300).contains(&resp.status) {
            let err_hint = account_store::error_code(&resp.body);
            return Ok(ProviderTestResult {
                ok: false,
                latency_ms,
                models: Vec::new(),
                message: format!("HTTP {} {}", resp.status, err_hint).trim().to_string(),
            });
        }
        let mut models = Vec::new();
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.body) {
            if let Some(arr) = v.get("data").or_else(|| v.get("models")).and_then(|x| x.as_array()) {
                for item in arr.iter().take(60) {
                    let mid = item
                        .get("id")
                        .or_else(|| item.get("name"))
                        .and_then(|x| x.as_str())
                        .or_else(|| item.as_str());
                    if let Some(m) = mid {
                        let s = m.trim().to_string();
                        if !s.is_empty() && !models.contains(&s) {
                            models.push(s);
                        }
                    }
                }
            }
        }
        let msg = if models.is_empty() {
            format!("连通成功（{} ms）", latency_ms)
        } else {
            format!("连通成功（{} ms，检测到 {} 个模型）", latency_ms, models.len())
        };
        Ok(ProviderTestResult {
            ok: true,
            latency_ms,
            models,
            message: msg,
        })
    })
    .await
}

/// 只读地从 cc-switch 导入中转供应商（支持 Claude 与 Codex），返回新增数量。
#[tauri::command]
pub async fn import_cc_switch_providers(state: State<'_, AppState>, client: AccountClient) -> Result<usize, String> {
    let m = Manager::of(&state, client);
    let db = CcSwitchState::detect().database;
    run_blocking(move || match m {
        Manager::Claude(cl) => cl.import_cc_switch(db.as_deref()),
        Manager::Codex(cx) => cx.import_cc_switch(db.as_deref()),
        Manager::Antigravity(_) => Err("Antigravity 不使用 cc-switch 中转配置".into()),
    })
    .await
}

/// 兼容旧命令：只读地从 cc-switch 导入 Claude 中转供应商，返回新增数量。
#[tauri::command]
pub async fn import_cc_switch_claude_providers(state: State<'_, AppState>) -> Result<usize, String> {
    let p = &state.paths;
    let m = ClaudeAccountManager::new(&p.claude_home(), &p.claude_json(), &p.data, &p.backups);
    run_blocking(move || m.import_cc_switch(CcSwitchState::detect().database.as_deref())).await
}

/// 从本机 `~/.cockpit_tools` 一键导入对应平台的账号、标签与备注。
#[tauri::command]
pub async fn import_cockpit_tools_accounts(state: State<'_, AppState>, client: AccountClient) -> Result<TokenImportResult, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let dirs = account_store::cockpit_data_dirs();
        let (ids, failures) = match &m {
            Manager::Antigravity(ag) => {
                let r = ag.import_cockpit_tools_from(&dirs)?;
                (r.ids, r.failures)
            }
            Manager::Codex(cx) => {
                let r = cx.import_cockpit_tools_from(&dirs)?;
                (r.ids, r.failures)
            }
            Manager::Claude(cl) => {
                let r = cl.import_cockpit_tools_from(&dirs)?;
                (r.ids, r.failures)
            }
        };
        for id in &ids {
            let _ = m.refresh_quota(id);
        }
        Ok(TokenImportResult {
            imported: ids.len(),
            failures,
        })
    })
    .await
}

/// 导出选中或全部账号 JSON；若提供 `path` 则同步写盘。
#[tauri::command]
pub async fn export_accounts_json(
    state: State<'_, AppState>,
    client: AccountClient,
    ids: Option<Vec<String>>,
    path: Option<String>,
) -> Result<String, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let json_text = m.export_json(ids.as_deref())?;
        if let Some(raw_path) = path {
            let norm = crate::util::normalize_windows_path(&raw_path);
            account_store::atomic_write(Path::new(&norm), json_text.as_bytes())?;
        }
        Ok(json_text)
    })
    .await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLaunchRequest {
    pub client: AccountClient,
    pub account_id: Option<String>,
    pub tool_id: String,
    pub folder_path: Option<String>,
    pub terminal: Option<String>,
    pub custom_command: Option<String>,
}

/// 先校验启动目标与工作目录，按需安全切换指定账号后，再通过桌面端、CLI 或编辑器启动。
#[tauri::command]
pub async fn launch_account_target(
    state: State<'_, AppState>,
    request: AccountLaunchRequest,
) -> Result<(), String> {
    let service = QuickLaunchService::new(&state.paths.data);
    let m = Manager::of(&state, request.client);
    run_blocking(move || {
        service.validate_launch_request(&request.tool_id, request.folder_path.as_deref())?;

        if let Some(id) = request
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let cur_state = m.state()?;
            let already_current = cur_state
                .accounts
                .iter()
                .any(|acc| acc.id == id && acc.is_current);

            if !already_current {
                match &m {
                    Manager::Codex(cm) => {
                        let controller = CodexClientController::new();
                        if request.tool_id == "codex_app" && controller.is_running() {
                            controller.stop(15.0)?;
                        }
                        cm.switch(id)?;
                    }
                    Manager::Claude(cm) => {
                        let controller = ClaudeClientController::new();
                        if request.tool_id == "claude_app" && controller.is_running() {
                            controller.stop(15.0)?;
                        }
                        cm.switch(id)?;
                    }
                    Manager::Antigravity(am) => {
                        let has_legacy_db =
                            crate::services::antigravity_accounts::default_state_db().is_file();
                        let controller = AntigravityClientController::new();
                        let running = controller.is_running();
                        let launching_desktop = request.tool_id == "antigravity_app";
                        if launching_desktop && running {
                            controller.stop(15.0)?;
                        } else if !launching_desktop && has_legacy_db && running {
                            return Err(
                                "Antigravity 正在运行，请先关闭它，或选择以 Antigravity 桌面端启动"
                                    .into(),
                            );
                        }
                        am.switch(id)?;
                    }
                }
            }
        }

        service.launch_tool_with_options(
            &request.tool_id,
            request.folder_path.as_deref(),
            request.terminal.as_deref(),
            request.custom_command.as_deref(),
        )
    })
    .await
}

/// 在指定项目文件夹打开交互终端并启动对应 CLI（`codex` / `claude` / `agy`）；
/// 若传入未激活的账号 `id` 会先切换为该账号再启动。
#[tauri::command]
pub async fn launch_account_cli(
    state: State<'_, AppState>,
    client: AccountClient,
    id: Option<String>,
    folder_path: String,
) -> Result<(), String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let dir = PathBuf::from(crate::util::normalize_windows_path(&folder_path));
        if !dir.is_dir() {
            return Err(format!("目标文件夹不存在：{}", dir.display()));
        }
        if let Some(target_id) = id {
            let cur_state = m.state()?;
            let already_current = cur_state
                .accounts
                .iter()
                .find(|a| a.id == target_id)
                .is_some_and(|a| a.is_current);
            if !already_current {
                match &m {
                    Manager::Codex(cx) => cx.switch(&target_id)?,
                    Manager::Claude(cl) => cl.switch(&target_id)?,
                    Manager::Antigravity(ag) => ag.switch(&target_id)?,
                }
            }
        }
        spawn_cli_in_folder(client, &dir)
    })
    .await
}

fn find_agy_cli() -> Option<PathBuf> {
    crate::services::quick_launch::find_agy_cli_executable()
}

fn find_claude_cli() -> Option<PathBuf> {
    crate::services::quick_launch::find_claude_cli_executable()
}

fn spawn_cli_in_folder(client: AccountClient, dir: &Path) -> Result<(), String> {
    let exe_str = match client {
        AccountClient::Codex => find_codex_cli().to_string_lossy().to_string(),
        AccountClient::Claude => find_claude_cli()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "claude".into()),
        AccountClient::Antigravity => find_agy_cli()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "agy".into()),
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x00000010;
        let escaped = exe_str.replace('\'', "''");
        let ps_cmd = format!("& '{}'", escaped);
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoExit", "-Command", &ps_cmd])
            .current_dir(dir)
            .creation_flags(CREATE_NEW_CONSOLE);
        if client == AccountClient::Claude {
            for key in crate::services::claude_accounts::PROVIDER_KEYS {
                cmd.env_remove(key);
            }
        }
        cmd.spawn()
            .map_err(|e| format!("启动终端失败：{}", e))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new(&exe_str);
        cmd.current_dir(dir);
        cmd.spawn()
            .map_err(|e| format!("启动命令行失败：{}", e))?;
        Ok(())
    }
}

/// 最新安装的 Codex CLI：桌面版自带的 `%LOCALAPPDATA%\OpenAI\Codex\bin\<版本>\codex.exe`，否则用 PATH 中的 `codex`。
fn find_codex_cli() -> PathBuf {
    crate::services::quick_launch::find_codex_cli_executable()
        .unwrap_or_else(|| PathBuf::from("codex"))
}

fn codex_login_command(codex_home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(find_codex_cli());
    command.args(["login", "-c", "cli_auth_credentials_store=\"file\""]).env("CODEX_HOME", codex_home);
    command
}

/// Claude Code：优先已安装的原生 / npm / 内置可执行文件，否则通过 `cmd /c claude` 调用。
fn claude_login_command(config_dir: &Path) -> std::process::Command {
    let mut command = match find_claude_cli() {
        Some(exe) => std::process::Command::new(exe),
        None => {
            let mut c = std::process::Command::new("cmd");
            c.args(["/c", "claude"]);
            c
        }
    };
    command.args(["auth", "login", "--claudeai"]).env("CLAUDE_CONFIG_DIR", config_dir);
    for key in crate::services::claude_accounts::PROVIDER_KEYS {
        command.env_remove(key);
    }
    command
}

/// 运行官方登录命令并等待它写出 `expected` 文件。
fn run_login(mut command: std::process::Command, expected: &Path) -> Result<(), String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| format!("无法启动官方命令行工具：{}（请确认已安装）", e))?;
    LOGIN_PID.store(child.id(), Ordering::SeqCst);
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed().as_secs() > LOGIN_TIMEOUT_SECS => {
                let _ = crate::services::process_util::kill_tree(child.id());
                LOGIN_PID.store(0, Ordering::SeqCst);
                return Err("等待浏览器登录超时，请重试".into());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(300)),
            Err(e) => return Err(format!("等待登录命令失败：{}", e)),
        }
    };
    if LOGIN_PID.swap(0, Ordering::SeqCst) == 0 {
        return Err("已取消登录".into());
    }
    if !status.success() || !expected.is_file() {
        return Err("登录没有完成（浏览器中未完成授权或已关闭）".into());
    }
    Ok(())
}
