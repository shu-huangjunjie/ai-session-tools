use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::State;

use super::run_blocking;
use crate::services::{
    dispatch_to_prism_studio, get_prism_status, resolve_vault_conversation, DataService,
    PrismBridgeAction, PrismStudioStatus, QuickLaunchService, QuickLaunchTool, VSCodeLauncher,
    VaultFocusPayload,
};
use crate::util::normalize_windows_path;
use crate::AppState;

#[derive(serde::Serialize)]
pub struct AppPathsInfo {
    pub root: String,
    pub data: String,
    pub database: String,
    pub backups: String,
    pub logs: String,
}

#[tauri::command]
pub async fn backup_database(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.database.clone();
    run_blocking(move || {
        let path = DataService::new(&db).backup()?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
}

#[tauri::command]
pub async fn export_markdown(
    state: State<'_, AppState>,
    conversation_id: String,
    target_path: String,
) -> Result<(), String> {
    let db = state.database.clone();
    run_blocking(move || {
        DataService::new(&db).export_markdown(&conversation_id, Path::new(&target_path))
    })
    .await
}

#[tauri::command]
pub async fn batch_export_markdown(
    state: State<'_, AppState>,
    conversation_ids: Vec<String>,
    target_dir: String,
) -> Result<usize, String> {
    let db = state.database.clone();
    run_blocking(move || {
        DataService::new(&db).batch_export_markdown(&conversation_ids, Path::new(&target_dir))
    })
    .await
}

#[tauri::command]
pub async fn export_json(
    state: State<'_, AppState>,
    conversation_id: String,
    target_path: String,
) -> Result<(), String> {
    let db = state.database.clone();
    run_blocking(move || {
        DataService::new(&db).export_json(&conversation_id, Path::new(&target_path))
    })
    .await
}

#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    run_blocking(move || {
        let target = PathBuf::from(normalize_windows_path(&path));
        if !target.exists() {
            return Err(format!("目标路径不存在：{}", target.display()));
        }
        let mut command = Command::new("explorer.exe");
        if target.is_file() {
            command.arg(format!("/select,{}", target.display()));
        } else {
            command.arg(&target);
        }
        command
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn open_vscode(path: String) -> Result<String, String> {
    run_blocking(move || {
        let target = PathBuf::from(normalize_windows_path(&path));
        let exe = VSCodeLauncher::new().open_project(&target)?;
        Ok(exe.to_string_lossy().to_string())
    })
    .await
}

#[tauri::command]
pub fn get_app_paths(state: State<'_, AppState>) -> AppPathsInfo {
    AppPathsInfo {
        root: state.paths.root.to_string_lossy().to_string(),
        data: state.paths.data.to_string_lossy().to_string(),
        database: state.paths.database.to_string_lossy().to_string(),
        backups: state.paths.backups.to_string_lossy().to_string(),
        logs: state.paths.logs.to_string_lossy().to_string(),
    }
}

/// Shows a Windows Forms dialog through PowerShell. Values are passed in
/// environment variables, so paths and titles never become part of the script.
/// The hidden top-most owner keeps the dialog in front of the app window.
#[cfg(target_os = "windows")]
fn run_dialog(dialog_setup: &str, result_expr: &str, env: &[(&str, String)]) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         Add-Type -AssemblyName System.Windows.Forms; \
         [System.Windows.Forms.Application]::EnableVisualStyles(); \
         $owner = New-Object System.Windows.Forms.Form -Property @{{ TopMost = $true; ShowInTaskbar = $false }}; \
         {dialog_setup} \
         if ($dialog.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) {{ \
             [Console]::WriteLine({result_expr}) \
         }}; \
         $owner.Dispose()"
    );
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|e| format!("无法打开系统对话框：{}", e))?;
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!result.is_empty()).then_some(result))
}

fn initial_dir(dir: Option<String>) -> String {
    dir.map(|d| normalize_windows_path(&d))
        .filter(|d| !d.is_empty() && Path::new(d).is_dir())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pick_save_path(
    default_filename: String,
    filter_name: String,
    filter_ext: String,
    initial_dir: Option<String>,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let ext = filter_ext.trim_start_matches('.').replace(['|', ';', '*'], "");
        let filter = format!(
            "{} (*.{ext})|*.{ext}|所有文件 (*.*)|*.*",
            filter_name.replace('|', " ")
        );
        let env = [
            ("CV_DIALOG_FILE", crate::util::sanitize_file_name(&default_filename, "export")),
            ("CV_DIALOG_FILTER", filter),
            ("CV_DIALOG_EXT", ext.clone()),
            ("CV_DIALOG_DIR", self::initial_dir(initial_dir)),
        ];
        run_blocking(move || {
            run_dialog(
                "$dialog = New-Object System.Windows.Forms.SaveFileDialog; \
                 $dialog.Title = '选择保存位置'; \
                 $dialog.Filter = $env:CV_DIALOG_FILTER; \
                 $dialog.DefaultExt = $env:CV_DIALOG_EXT; \
                 $dialog.AddExtension = $true; \
                 $dialog.OverwritePrompt = $true; \
                 $dialog.FileName = $env:CV_DIALOG_FILE; \
                 if ($env:CV_DIALOG_DIR) { $dialog.InitialDirectory = $env:CV_DIALOG_DIR };",
                "$dialog.FileName",
                &env,
            )
        })
        .await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (default_filename, filter_name, filter_ext, initial_dir);
        Ok(None)
    }
}

#[tauri::command]
pub async fn pick_open_path(initial_dir: Option<String>) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let env = [("CV_DIALOG_DIR", self::initial_dir(initial_dir))];
        run_blocking(move || {
            run_dialog(
                "$dialog = New-Object System.Windows.Forms.OpenFileDialog; \
                 $dialog.Title = '选择要导入的会话文件'; \
                 $dialog.Filter = '会话文件 (*.zip;*.json;*.jsonl;*.ndjson;*.md;*.markdown)|*.zip;*.json;*.jsonl;*.ndjson;*.md;*.markdown|所有文件 (*.*)|*.*'; \
                 $dialog.Multiselect = $false; \
                 if ($env:CV_DIALOG_DIR) { $dialog.InitialDirectory = $env:CV_DIALOG_DIR };",
                "$dialog.FileName",
                &env,
            )
        })
        .await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = initial_dir;
        Ok(None)
    }
}

/// 选择要导入的 Codex 登录文件 `auth.json`。
#[tauri::command]
pub async fn pick_auth_json_path(initial_dir: Option<String>) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let env = [("CV_DIALOG_DIR", self::initial_dir(initial_dir))];
        run_blocking(move || {
            run_dialog(
                "$dialog = New-Object System.Windows.Forms.OpenFileDialog; \
                 $dialog.Title = '选择 Codex 登录文件 auth.json'; \
                 $dialog.Filter = 'Codex 登录文件 (auth.json)|auth.json;*.json|所有文件 (*.*)|*.*'; \
                 $dialog.Multiselect = $false; \
                 if ($env:CV_DIALOG_DIR) { $dialog.InitialDirectory = $env:CV_DIALOG_DIR };",
                "$dialog.FileName",
                &env,
            )
        })
        .await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = initial_dir;
        Ok(None)
    }
}

#[tauri::command]
pub async fn pick_folder_path(
    window: tauri::WebviewWindow,
    initial_dir: Option<String>,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let owner = window.hwnd().map_err(|e| format!("无法获取应用窗口：{e}"))?.0 as isize;
        let directory = self::initial_dir(initial_dir);
        run_blocking(move || crate::folder_picker::pick_folder(owner, directory))
        .await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, initial_dir);
        Ok(None)
    }
}

#[tauri::command]
pub async fn get_prism_studio_status() -> Result<PrismStudioStatus, String> {
    run_blocking(|| Ok(get_prism_status())).await
}

#[tauri::command]
pub async fn open_in_prism_studio(
    path: String,
    mode: Option<String>,
    line: Option<u32>,
    project_path: Option<String>,
) -> Result<String, String> {
    run_blocking(move || {
        let normalized = normalize_windows_path(&path);
        let target = PathBuf::from(&normalized);
        if !target.exists() {
            return Err(format!("目标路径不存在：{}", target.display()));
        }
        let resolved_proj = project_path
            .map(|p| normalize_windows_path(&p))
            .filter(|p| !p.is_empty());

        let action = if target.is_file() || mode.as_deref() == Some("file") {
            PrismBridgeAction::OpenFile {
                path: normalized,
                line,
                project_path: resolved_proj,
            }
        } else {
            let ws_mode = match mode.as_deref() {
                Some("parallel-root") => "parallel-root".to_string(),
                _ => "new-workspace".to_string(),
            };
            PrismBridgeAction::OpenWorkspace {
                path: normalized,
                mode: ws_mode,
            }
        };
        dispatch_to_prism_studio(action)
    })
    .await
}

#[tauri::command]
pub async fn send_code_to_prism_studio(
    language: String,
    code: String,
    title: Option<String>,
    project_path: Option<String>,
    auto_run: Option<bool>,
) -> Result<String, String> {
    run_blocking(move || {
        let resolved_proj = project_path
            .map(|p| normalize_windows_path(&p))
            .filter(|p| !p.is_empty() && Path::new(p).is_dir());
        let action = PrismBridgeAction::OpenCodeSnippet {
            language: language.trim().to_string(),
            code,
            title: title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "AI会话代码实验".to_string()),
            project_path: resolved_proj,
            auto_run: auto_run.unwrap_or(false),
        };
        dispatch_to_prism_studio(action)
    })
    .await
}

#[tauri::command]
pub async fn open_conversation_in_prism_studio(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<String, String> {
    let db = state.database.clone();
    run_blocking(move || {
        let (conv, markdown) = DataService::new(&db).render_conversation_markdown(&conversation_id)?;
        let resolved_proj = conv
            .project_path
            .map(|p| normalize_windows_path(&p))
            .filter(|p| !p.is_empty() && Path::new(p).is_dir());
        let action = PrismBridgeAction::OpenMarkdownNote {
            title: conv.title,
            markdown,
            project_path: resolved_proj,
        };
        dispatch_to_prism_studio(action)
    })
    .await
}

#[tauri::command]
pub fn drain_vault_bridge_focus(state: State<'_, AppState>) -> Vec<VaultFocusPayload> {
    state.bridge_queue.drain()
}

#[tauri::command]
pub async fn resolve_conversation_link(
    state: State<'_, AppState>,
    id_or_source_id: String,
) -> Result<Option<VaultFocusPayload>, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Ok(
            resolve_vault_conversation(&db, &id_or_source_id).map(|(id, source_kind)| {
                VaultFocusPayload {
                    conversation_id: Some(id),
                    source_kind: Some(source_kind),
                    project_path: None,
                }
            }),
        )
    })
    .await
}

#[tauri::command]
pub async fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    let trimmed = url.trim().to_string();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("仅允许打开安全的 http/https 网页链接".to_string());
    }
    #[allow(deprecated)]
    app.shell()
        .open(&trimmed, None)
        .map_err(|e| format!("无法调用系统浏览器打开链接: {e}"))
}

/// 选择外部工具的可执行文件（`.exe` / `.cmd` / `.bat`），用于自定义快捷启动路径。
#[tauri::command]
pub async fn pick_executable_path(initial_dir: Option<String>) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let env = [("CV_DIALOG_DIR", self::initial_dir(initial_dir))];
        run_blocking(move || {
            run_dialog(
                "$dialog = New-Object System.Windows.Forms.OpenFileDialog; \
                 $dialog.Title = '选择可执行程序'; \
                 $dialog.Filter = '可执行程序 (*.exe;*.cmd;*.bat)|*.exe;*.cmd;*.bat|所有文件 (*.*)|*.*'; \
                 $dialog.Multiselect = $false; \
                 if ($env:CV_DIALOG_DIR) { \
                   $p = $env:CV_DIALOG_DIR; \
                   if (Test-Path -LiteralPath $p -PathType Leaf) { $p = Split-Path -Parent $p }; \
                   if (Test-Path -LiteralPath $p -PathType Container) { $dialog.InitialDirectory = $p }; \
                 };",
                "$dialog.FileName",
                &env,
            )
        })
        .await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = initial_dir;
        Ok(None)
    }
}

/// 扫描本机已安装的 AI 客户端、CLI、编辑器与终端，并返回运行状态及自定义路径。
#[tauri::command]
pub async fn list_quick_launch_tools(
    state: State<'_, AppState>,
) -> Result<Vec<QuickLaunchTool>, String> {
    let service = QuickLaunchService::new(&state.paths.data);
    run_blocking(move || service.list_tools()).await
}

/// 通过工具 ID 快速启动对应桌面客户端、交互式 CLI 或终端（可选附加工作目录、终端类型与 CLI 自定义命令）。
#[tauri::command]
pub async fn launch_quick_tool(
    state: State<'_, AppState>,
    tool_id: String,
    folder_path: Option<String>,
    terminal: Option<String>,
    custom_command: Option<String>,
) -> Result<(), String> {
    let service = QuickLaunchService::new(&state.paths.data);
    run_blocking(move || {
        service.launch_tool_with_options(
            &tool_id,
            folder_path.as_deref(),
            terminal.as_deref(),
            custom_command.as_deref(),
        )
    })
    .await
}

/// 设置或清除指定快捷启动工具的自定义可执行文件路径。
#[tauri::command]
pub async fn set_quick_launch_custom_path(
    state: State<'_, AppState>,
    tool_id: String,
    custom_path: Option<String>,
) -> Result<Vec<QuickLaunchTool>, String> {
    let service = QuickLaunchService::new(&state.paths.data);
    run_blocking(move || service.set_custom_path(&tool_id, custom_path)).await
}
