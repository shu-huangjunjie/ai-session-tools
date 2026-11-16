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
    pub bridge_queue: Arc<services::VaultBridgeQueue>,
}

fn initialize(initial_focus: Option<services::VaultFocusPayload>) -> Result<AppState, String> {
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

    // Repair previously imported placeholders using Codex's current display names.
    let _ = services::CodexRecoveryService::new(&database, Some(&paths.codex_home()))
        .refresh_existing_titles();

    let bridge_queue = Arc::new(services::VaultBridgeQueue::default());
    if let Some(mut focus) = initial_focus {
        if let Some(ref cid) = focus.conversation_id.clone() {
            if let Some((resolved_id, sk)) = services::resolve_vault_conversation(&database, cid) {
                focus.conversation_id = Some(resolved_id);
                focus.source_kind = Some(sk);
            }
        }
        bridge_queue.push(focus);
    }

    Ok(AppState {
        paths,
        database,
        bridge_queue,
    })
}

fn handle_cli_headless_or_handoff() -> Result<Option<services::VaultFocusPayload>, ()> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    let mut handoff_focus: Option<services::VaultFocusPayload> = None;

    while i < args.len() {
        if args[i] == "--headless-bridge-query" && i + 1 < args.len() {
            let req_path = std::path::PathBuf::from(&args[i + 1]);
            let resp = match services::load_handoff_file::<services::VaultHeadlessOrRpcRequest>(&req_path) {
                Some(req) => match services::discover_best_vault_paths() {
                    Ok(paths) => match Database::new(&paths.database, &paths.backups) {
                        Ok(db) => services::execute_vault_data_query(&db, &req),
                        Err(e) => services::BridgeResponse {
                            ok: false,
                            error: Some(format!("打开会话数据库失败: {e}")),
                            data: None,
                        },
                    },
                    Err(e) => services::BridgeResponse {
                        ok: false,
                        error: Some(format!("发现数据目录失败: {e}")),
                        data: None,
                    },
                },
                None => services::BridgeResponse {
                    ok: false,
                    error: Some("无法读取无界面查询参数".to_string()),
                    data: None,
                },
            };
            if let Ok(json) = serde_json::to_string(&resp) {
                println!("{json}");
            }
            return Err(());
        } else if args[i] == "--bridge-handoff" && i + 1 < args.len() {
            let handoff_path = std::path::PathBuf::from(&args[i + 1]);
            handoff_focus = services::load_handoff_file::<services::VaultFocusPayload>(&handoff_path);
            i += 2;
            continue;
        }
        i += 1;
    }

    let forward_payload = handoff_focus.clone().unwrap_or(services::VaultFocusPayload {
        conversation_id: None,
        source_kind: None,
        project_path: None,
    });
    if services::try_forward_to_running_vault(Some(forward_payload)) {
        return Err(());
    }

    Ok(handoff_focus)
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
    let initial_focus = match handle_cli_headless_or_handoff() {
        Ok(f) => f,
        Err(()) => return,
    };

    let state = match initialize(initial_focus) {
        Ok(state) => state,
        Err(message) => {
            report_startup_error(&message);
            return;
        }
    };

    let bridge_paths = state.paths.clone();
    let bridge_db = state.database.clone();
    let bridge_queue = state.bridge_queue.clone();

    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .setup(move |app| {
            services::start_vault_bridge_server(
                app.handle().clone(),
                &bridge_paths,
                bridge_db,
                bridge_queue,
            );
            Ok(())
        })
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
            commands::handoff_conversation,

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

            // Skills
            commands::list_skills,
            commands::read_skill_file,
            commands::create_skill,
            commands::set_skill_enabled,
            commands::mount_skill,
            commands::link_skill_to_client,
            commands::delete_skill,

            // MCP servers
            commands::list_mcp_servers,
            commands::upsert_mcp_server,
            commands::set_mcp_enabled,
            commands::delete_mcp_server,
            commands::copy_mcp_server,

            // Accounts (Codex / Claude / Antigravity, kept apart)
            commands::list_accounts,
            commands::save_current_account,
            commands::import_account_file,
            commands::submit_account_login_callback,
            commands::login_account,
            commands::start_codex_device_login,
            commands::complete_codex_device_login,
            commands::import_account_tokens,
            commands::cancel_account_login,
            commands::switch_account,
            commands::delete_account,
            commands::update_account_meta,
            commands::batch_update_account_tags,
            commands::batch_delete_accounts,
            commands::refresh_account_quota,
            commands::refresh_all_account_quotas,
            commands::upsert_claude_provider,
            commands::upsert_codex_provider,
            commands::test_provider_connection,
            commands::import_cc_switch_providers,
            commands::import_cc_switch_claude_providers,
            commands::import_cockpit_tools_accounts,
            commands::export_accounts_json,
            commands::launch_account_cli,
            commands::launch_account_target,

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
            commands::get_prism_studio_status,
            commands::open_in_prism_studio,
            commands::send_code_to_prism_studio,
            commands::open_conversation_in_prism_studio,
            commands::drain_vault_bridge_focus,
            commands::resolve_conversation_link,
            commands::open_external_url,
            commands::get_app_paths,
            commands::pick_save_path,
            commands::pick_open_path,
            commands::pick_auth_json_path,
            commands::pick_folder_path,
            commands::pick_executable_path,
            commands::list_quick_launch_tools,
            commands::launch_quick_tool,
            commands::set_quick_launch_custom_path,
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
