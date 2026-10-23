use serde_json::Value;
use tauri::State;

use super::run_blocking;
use crate::services::{CcSwitchState, McpClient, McpCopyOutcome, McpLocation, McpManager};
use crate::AppState;

fn manager(state: &AppState) -> McpManager {
    McpManager::new(
        &state.paths.claude_json(),
        &state.paths.codex_home(),
        &state.paths.data,
        &state.paths.backups,
    )
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpLocation>, String> {
    let m = manager(&state);
    run_blocking(move || Ok(m.list(&CcSwitchState::detect()))).await
}

/// `original` 为空时新建，否则编辑（可改名）。
#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    client: McpClient,
    original: Option<String>,
    name: String,
    spec: Value,
) -> Result<(), String> {
    let m = manager(&state);
    run_blocking(move || m.upsert(client, original.as_deref(), name.trim(), &spec)).await
}

#[tauri::command]
pub async fn set_mcp_enabled(
    state: State<'_, AppState>,
    client: McpClient,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    let m = manager(&state);
    run_blocking(move || m.set_enabled(client, &name, enabled)).await
}

#[tauri::command]
pub async fn delete_mcp_server(state: State<'_, AppState>, client: McpClient, name: String) -> Result<(), String> {
    let m = manager(&state);
    run_blocking(move || m.delete(client, &name)).await
}

#[tauri::command]
pub async fn copy_mcp_server(
    state: State<'_, AppState>,
    from: McpClient,
    name: String,
    to: McpClient,
    overwrite: bool,
) -> Result<McpCopyOutcome, String> {
    let m = manager(&state);
    run_blocking(move || m.copy(from, &name, to, overwrite)).await
}
