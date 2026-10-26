//! 账号管理命令。每个命令都带 `client`，三个客户端（Codex、Claude、Antigravity）的账号分开保存、分开切换。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::Serialize;
use tauri::{Emitter, State};

use super::run_blocking;
use crate::services::{
    AccountClient, AccountsState, AntigravityAccountManager, AntigravityClientController, CcSwitchState, ClaudeAccountManager,
    CodexAccountManager, CodexClientController, ProviderInput,
};
use crate::AppState;

/// 正在进行的官方登录进程，供取消使用。
static LOGIN_PID: AtomicU32 = AtomicU32::new(0);
/// Antigravity 的浏览器授权没有子进程，取消时置位，等待授权的循环看到后退出。
static LOGIN_CANCELLED: AtomicBool = AtomicBool::new(false);
/// 等待浏览器登录的最长时间。
const LOGIN_TIMEOUT_SECS: u64 = 600;

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
            AccountClient::Antigravity => Manager::Antigravity(AntigravityAccountManager::new(
                &crate::services::antigravity_accounts::default_state_db(),
                &p.data,
                &p.backups,
            )),
        }
    }

    fn state(&self) -> Result<AccountsState, String> {
        match self {
            Manager::Codex(m) => m.state(),
            Manager::Claude(m) => m.state(),
            Manager::Antigravity(m) => m.state(),
        }
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
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>, client: AccountClient) -> Result<AccountsState, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.state()).await
}

#[tauri::command]
pub async fn save_current_account(state: State<'_, AppState>, client: AccountClient) -> Result<String, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.save_current()).await
}

/// 导入 Codex 的 `auth.json`。
#[tauri::command]
pub async fn import_account_file(state: State<'_, AppState>, client: AccountClient, path: String) -> Result<String, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || match m {
        Manager::Codex(m) => m.import_file(Path::new(&crate::util::normalize_windows_path(&path))),
        _ => Err("只有 Codex 支持导入登录文件".into()),
    })
    .await
}

/// 在浏览器里登录新账号并保存，当前登录不受影响：Codex、Claude 用官方 CLI（`codex login` /
/// `claude auth login`）在独立目录里登录；Antigravity 直接走 Google 授权，授权链接通过
/// `account-login-url` 事件发给界面（浏览器没自动打开时可手动打开）。
#[tauri::command]
pub async fn login_account(app: tauri::AppHandle, state: State<'_, AppState>, client: AccountClient) -> Result<String, String> {
    let m = Manager::of(&state, client);
    let dir = state.paths.data.join("accounts").join(format!("login-{}", uuid::Uuid::new_v4()));
    LOGIN_CANCELLED.store(false, Ordering::SeqCst);
    run_blocking(move || {
        if let Manager::Antigravity(m) = &m {
            let id = m.login_in_browser(&|url| { let _ = app.emit("account-login-url", url); }, &|| LOGIN_CANCELLED.load(Ordering::SeqCst))?;
            // 额度查不到不影响账号已添加。
            let _ = m.refresh_quota(&id);
            return Ok(id);
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建临时目录：{}", e))?;
        let result = match &m {
            Manager::Codex(m) => run_login(codex_login_command(&dir), &dir.join("auth.json")).and_then(|_| m.import_login_dir(&dir)),
            Manager::Claude(m) => run_login(claude_login_command(&dir), &dir.join(".credentials.json")).and_then(|_| m.import_login_dir(&dir)),
            Manager::Antigravity(_) => unreachable!(),
        };
        let _ = std::fs::remove_dir_all(&dir);
        result
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct TokenImportResult {
    pub imported: usize,
    pub failures: Vec<String>,
}

/// 导入 Antigravity 的 refresh token（每行一个或账号 JSON），导入后查询一次额度。
#[tauri::command]
pub async fn import_account_tokens(state: State<'_, AppState>, client: AccountClient, text: String) -> Result<TokenImportResult, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || match &m {
        Manager::Antigravity(m) => {
            let result = m.import_tokens(&text)?;
            for id in &result.ids {
                let _ = m.refresh_quota(id);
            }
            Ok(TokenImportResult { imported: result.ids.len(), failures: result.failures })
        }
        _ => Err("只有 Antigravity 支持导入 refresh token".into()),
    })
    .await
}

#[tauri::command]
pub async fn cancel_account_login() -> Result<(), String> {
    LOGIN_CANCELLED.store(true, Ordering::SeqCst);
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
/// - Antigravity：运行时必须关闭才能切换；`restart` 为真时代为关闭并重新打开，否则报错。
/// - Claude：新开的 Claude Code 会话自动使用新账号，不重启任何程序。
#[tauri::command]
pub async fn switch_account(state: State<'_, AppState>, client: AccountClient, id: String, restart: bool) -> Result<bool, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || match m {
        Manager::Codex(m) => {
            let controller = CodexClientController::new();
            let restart = restart && controller.is_running();
            if restart {
                controller.stop(15.0)?;
            }
            let result = m.switch(&id);
            if restart {
                controller.start(15.0)?;
            }
            result.map(|_| restart)
        }
        Manager::Claude(m) => m.switch(&id).map(|_| false),
        Manager::Antigravity(m) => {
            let controller = AntigravityClientController::new();
            let running = controller.is_running();
            if running && !restart {
                return Err("Antigravity 正在运行，请先关闭它，或勾选“切换后重启”".into());
            }
            if running {
                controller.stop(15.0)?;
            }
            let result = m.switch(&id);
            if running {
                controller.start(15.0)?;
            }
            result.map(|_| running)
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
pub async fn refresh_account_quota(state: State<'_, AppState>, client: AccountClient, id: String) -> Result<(), String> {
    let m = Manager::of(&state, client);
    run_blocking(move || m.refresh_quota(&id)).await
}

#[derive(Serialize)]
pub struct QuotaRefreshFailure {
    pub id: String,
    pub error: String,
}

/// 依次刷新该客户端所有能查额度的账号，返回失败的账号。
#[tauri::command]
pub async fn refresh_all_account_quotas(state: State<'_, AppState>, client: AccountClient) -> Result<Vec<QuotaRefreshFailure>, String> {
    let m = Manager::of(&state, client);
    run_blocking(move || {
        let ids: Vec<String> = m.state()?.accounts.into_iter().filter(|a| a.has_quota).map(|a| a.id).collect();
        Ok(ids
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

/// 只读地从 cc-switch 导入 Claude 中转供应商，返回新增数量。
#[tauri::command]
pub async fn import_cc_switch_claude_providers(state: State<'_, AppState>) -> Result<usize, String> {
    let p = &state.paths;
    let m = ClaudeAccountManager::new(&p.claude_home(), &p.claude_json(), &p.data, &p.backups);
    run_blocking(move || m.import_cc_switch(CcSwitchState::detect().database.as_deref())).await
}

/// 最新安装的 Codex CLI：桌面版自带的 `%LOCALAPPDATA%\OpenAI\Codex\bin\<版本>\codex.exe`，否则用 PATH 中的 `codex`。
fn find_codex_cli() -> PathBuf {
    let newest = dirs::data_local_dir()
        .map(|d| d.join("OpenAI").join("Codex").join("bin"))
        .and_then(|bin| std::fs::read_dir(bin).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("codex.exe"))
        .filter(|p| p.is_file())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    newest.unwrap_or_else(|| PathBuf::from("codex"))
}

fn codex_login_command(codex_home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(find_codex_cli());
    command.args(["login", "-c", "cli_auth_credentials_store=\"file\""]).env("CODEX_HOME", codex_home);
    command
}

/// Claude Code：独立安装的 `~/.local/bin/claude.exe`，否则通过 `cmd /c claude` 使用 npm 安装的版本。
fn claude_login_command(config_dir: &Path) -> std::process::Command {
    let native = dirs::home_dir().map(|h| h.join(".local").join("bin").join("claude.exe")).filter(|p| p.is_file());
    let mut command = match native {
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
