pub mod commands;
pub mod domain;
pub mod importers;
pub mod paths;
pub mod persistence;
pub mod services;
pub mod util;

#[cfg(windows)]
mod folder_picker;

use std::sync::Arc;
use paths::AppPaths;
use persistence::{Database, Repository};

pub struct AppState {
    pub paths: AppPaths,
    pub database: Arc<Database>,
}

fn initialize() -> Result<AppState, String> {
    let paths =
        AppPaths::discover(None).map_err(|e| format!("无法创建应用数据目录：{}", e))?;
    let database = Database::new(&paths.database, &paths.backups).map_err(|e| {
        format!(
            "无法打开或升级会话数据库：\n{}\n\n{}",
            paths.database.display(),
            e
        )
    })?;
    let database = Arc::new(database);

    // Run crash recovery on stale streaming messages
    let _ = Repository::new(&database).recover_stale_requests();

    // Daily backup if needed
    let _ = database.backup_if_needed();

    Ok(AppState { paths, database })
}

/// A windowless release build has no console, so startup failures must be shown.
fn report_startup_error(message: &str) {
    let log = std::env::temp_dir().join("context-vault-startup-error.log");
    let _ = std::fs::write(&log, message);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Add-Type -AssemblyName PresentationFramework; \
                 [System.Windows.MessageBox]::Show($env:CV_MESSAGE, 'Context Vault 启动失败', 'OK', 'Error') | Out-Null",
            ])
            .env("CV_MESSAGE", message)
            .creation_flags(0x08000000)
            .status();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = match initialize() {
        Ok(state) => state,
        Err(message) => {
            report_startup_error(&message);
            return;
        }
    };

    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // Conversations
            commands::list_conversations,
            commands::get_conversation,
            commands::rename_conversation,
            commands::set_archived,
            commands::set_pinned,
            commands::list_folder_pins,
            commands::set_folder_pinned,
            commands::delete_conversation,
            commands::delete_conversations,
            commands::reset_conversations,
            commands::get_messages,
            commands::search_conversations,

            // Codex operations
            commands::scan_codex,
            commands::recover_codex_candidates,
            commands::preview_codex_publish,
            commands::publish_to_codex,
            commands::get_codex_projects,
            commands::get_codex_visibility,
            commands::is_codex_client_running,
            commands::stop_codex_client,
            commands::start_codex_client,

            // Antigravity operations
            commands::scan_antigravity,
            commands::recover_antigravity_candidates,
            commands::preview_antigravity_publish,
            commands::publish_to_antigravity,
            commands::get_antigravity_projects,
            commands::is_antigravity_client_running,
            commands::stop_antigravity_client,
            commands::start_antigravity_client,

            // Claude operations
            commands::scan_claude,
            commands::recover_claude_candidates,
            commands::preview_claude_publish,
            commands::publish_to_claude,
            commands::get_claude_projects,
            commands::is_claude_client_running,
            commands::stop_claude_client,
            commands::start_claude_client,

            // Importers
            commands::preview_import,
            commands::execute_import,

            // System operations
            commands::backup_database,
            commands::export_markdown,
            commands::batch_export_markdown,
            commands::export_json,
            commands::open_folder,
            commands::open_vscode,
            commands::get_app_paths,
            commands::pick_save_path,
            commands::pick_open_path,
            commands::pick_folder_path,
        ])
        .run(tauri::generate_context!())
    {
        report_startup_error(&format!(
            "运行 Context Vault 桌面端失败，这台电脑可能缺少 Microsoft Edge WebView2 运行时。\n\
             请从以下地址下载安装后重试：\n\
             https://developer.microsoft.com/microsoft-edge/webview2/\n\n{:?}",
            e
        ));
    }
}
