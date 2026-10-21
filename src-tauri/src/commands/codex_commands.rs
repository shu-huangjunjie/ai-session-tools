use std::sync::Arc;
use tauri::State;

use super::run_blocking;
use crate::persistence::{Database, Repository};
use crate::services::{
    CodexClientController, CodexClientPublisher, CodexProject, CodexPublishPreview,
    CodexPublishResult, CodexRecoveryReport, CodexRecoveryResult, CodexRecoveryService,
    CodexThreadCandidate, CodexVisibility,
};
use crate::AppState;

/// The frontend selects vault conversations; Codex identifies them by thread ID.
/// IDs that are not vault conversations are passed through as thread IDs.
fn resolve_thread_ids(db: &Database, ids: &[String]) -> Result<Vec<String>, String> {
    let mapping = Repository::new(db)
        .source_ids_for_conversations(CodexRecoveryService::SOURCE_TYPE, ids)
        .map_err(|e| e.to_string())?;
    let mut resolved: Vec<String> = ids
        .iter()
        .map(|id| mapping.get(id).cloned().unwrap_or_else(|| id.clone()))
        .collect();
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

#[tauri::command]
pub async fn scan_codex(state: State<'_, AppState>) -> Result<CodexRecoveryReport, String> {
    let db = state.database.clone();
    run_blocking(move || {
        let recovery = CodexRecoveryService::new(&db, None);
        let report = recovery.scan();
        recovery.sync_existing_titles(&report.candidates);
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn recover_codex_candidates(
    state: State<'_, AppState>,
    candidates: Vec<CodexThreadCandidate>,
) -> Result<CodexRecoveryResult, String> {
    let db = state.database.clone();
    run_blocking(move || Ok(CodexRecoveryService::new(&db, None).recover(&candidates))).await
}

#[tauri::command]
pub async fn preview_codex_publish(
    state: State<'_, AppState>,
    selected_thread_ids: Vec<String>,
) -> Result<CodexPublishPreview, String> {
    let db: Arc<Database> = state.database.clone();
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        let thread_ids = resolve_thread_ids(&db, &selected_thread_ids)?;
        CodexClientPublisher::new(None, &backups).preview(&thread_ids)
    })
    .await
}

#[tauri::command]
pub async fn publish_to_codex(
    state: State<'_, AppState>,
    selected_thread_ids: Vec<String>,
    purge_thread_ids: Vec<String>,
    auto_restart_client: bool,
) -> Result<CodexPublishResult, String> {
    let db = state.database.clone();
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        let thread_ids = resolve_thread_ids(&db, &selected_thread_ids)?;
        let purge_ids = resolve_thread_ids(&db, &purge_thread_ids)?;
        let publisher = CodexClientPublisher::new(None, &backups);
        // Validate before touching the running client.
        let preview = publisher.preview(&thread_ids)?;
        if preview.selected_thread_count == 0 {
            return Err(
                "所选会话均不在当前 Codex 索引中，未做任何修改。请先在 Codex 标签页勾选从 Codex 恢复的会话。"
                    .to_string(),
            );
        }

        let controller = CodexClientController::new();
        let was_running = controller.is_running();
        if was_running {
            controller.stop(15.0)?;
        }

        let result = publisher.publish(&thread_ids, &purge_ids);

        if was_running || auto_restart_client {
            let _ = controller.start(15.0);
        }
        result
    })
    .await
}

#[tauri::command]
pub async fn get_codex_projects(state: State<'_, AppState>) -> Result<Vec<CodexProject>, String> {
    let db = state.database.clone();
    run_blocking(move || Ok(CodexRecoveryService::new(&db, None).current_projects())).await
}

#[tauri::command]
pub async fn get_codex_visibility(state: State<'_, AppState>) -> Result<CodexVisibility, String> {
    let db = state.database.clone();
    run_blocking(move || CodexRecoveryService::new(&db, None).visibility()).await
}

#[tauri::command]
pub async fn is_codex_client_running() -> Result<bool, String> {
    run_blocking(|| Ok(CodexClientController::new().is_running())).await
}

#[tauri::command]
pub async fn stop_codex_client() -> Result<bool, String> {
    run_blocking(|| CodexClientController::new().stop(15.0)).await
}

#[tauri::command]
pub async fn start_codex_client() -> Result<(), String> {
    run_blocking(|| CodexClientController::new().start(15.0)).await
}
