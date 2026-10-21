use std::path::PathBuf;
use tauri::State;

use super::run_blocking;
use crate::services::{
    ClaudeCandidate, ClaudeClientController, ClaudeProject, ClaudePublishPreview,
    ClaudePublishResult, ClaudePublisher, ClaudeRecoveryReport, ClaudeRecoveryResult,
    ClaudeRecoveryService,
};
use crate::AppState;

fn claude_home(state: &AppState, custom_home: Option<String>) -> PathBuf {
    custom_home
        .filter(|h| !h.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.paths.claude_home())
}

#[tauri::command]
pub async fn scan_claude(
    state: State<'_, AppState>,
    custom_home: Option<String>,
) -> Result<ClaudeRecoveryReport, String> {
    let db = state.database.clone();
    let home = claude_home(&state, custom_home);
    run_blocking(move || {
        let recovery = ClaudeRecoveryService::new(&db, Some(&home));
        let report = recovery.scan();
        recovery.sync_existing_titles(&report.candidates);
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn recover_claude_candidates(
    state: State<'_, AppState>,
    candidates: Vec<ClaudeCandidate>,
    custom_home: Option<String>,
) -> Result<ClaudeRecoveryResult, String> {
    let db = state.database.clone();
    let home = claude_home(&state, custom_home);
    run_blocking(move || Ok(ClaudeRecoveryService::new(&db, Some(&home)).recover(&candidates)))
        .await
}

#[tauri::command]
pub async fn preview_claude_publish(
    state: State<'_, AppState>,
    selected_conversation_ids: Vec<String>,
    custom_home: Option<String>,
) -> Result<ClaudePublishPreview, String> {
    let db = state.database.clone();
    let home = claude_home(&state, custom_home);
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        ClaudePublisher::new(&db, Some(&home), &backups).preview(&selected_conversation_ids)
    })
    .await
}

#[tauri::command]
pub async fn publish_to_claude(
    state: State<'_, AppState>,
    selected_conversation_ids: Vec<String>,
    auto_restart: bool,
    custom_home: Option<String>,
) -> Result<ClaudePublishResult, String> {
    let db = state.database.clone();
    let home = claude_home(&state, custom_home);
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        ClaudePublisher::new(&db, Some(&home), &backups)
            .publish(&selected_conversation_ids, auto_restart)
    })
    .await
}

#[tauri::command]
pub async fn get_claude_projects(
    state: State<'_, AppState>,
    custom_home: Option<String>,
) -> Result<Vec<ClaudeProject>, String> {
    let db = state.database.clone();
    let home = claude_home(&state, custom_home);
    run_blocking(move || Ok(ClaudeRecoveryService::new(&db, Some(&home)).current_projects()))
        .await
}

#[tauri::command]
pub async fn is_claude_client_running() -> Result<bool, String> {
    run_blocking(|| Ok(ClaudeClientController::new().is_running())).await
}

#[tauri::command]
pub async fn stop_claude_client() -> Result<bool, String> {
    run_blocking(|| ClaudeClientController::new().stop(15.0)).await
}

#[tauri::command]
pub async fn start_claude_client() -> Result<(), String> {
    run_blocking(|| ClaudeClientController::new().start(15.0)).await
}
