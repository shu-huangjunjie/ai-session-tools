use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::State;

use super::run_blocking;
use crate::services::{DataService, VSCodeLauncher};
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
