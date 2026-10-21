use tauri::State;

use super::run_blocking;
use crate::services::{
    AntigravityCandidate, AntigravityClientController, AntigravityProject,
    AntigravityPublishPreview, AntigravityPublishResult, AntigravityPublisher,
    AntigravityRecoveryReport, AntigravityRecoveryResult, AntigravityRecoveryService,
};
use crate::AppState;

#[tauri::command]
pub async fn scan_antigravity(
    state: State<'_, AppState>,
) -> Result<AntigravityRecoveryReport, String> {
    let db = state.database.clone();
    let home = state.paths.antigravity_home();
    run_blocking(move || {
        let recovery = AntigravityRecoveryService::new(&db, Some(&home));
        let report = recovery.scan();
        recovery.sync_existing_titles(&report.candidates);
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn recover_antigravity_candidates(
    state: State<'_, AppState>,
    candidates: Vec<AntigravityCandidate>,
) -> Result<AntigravityRecoveryResult, String> {
    let db = state.database.clone();
    let home = state.paths.antigravity_home();
    run_blocking(move || {
        Ok(AntigravityRecoveryService::new(&db, Some(&home)).recover(&candidates))
    })
    .await
}

#[tauri::command]
pub async fn preview_antigravity_publish(
    state: State<'_, AppState>,
    selected_conversation_ids: Vec<String>,
) -> Result<AntigravityPublishPreview, String> {
    let db = state.database.clone();
    let home = state.paths.antigravity_home();
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        AntigravityPublisher::new(&db, Some(&home), &backups).preview(&selected_conversation_ids)
    })
    .await
}

#[tauri::command]
pub async fn publish_to_antigravity(
    state: State<'_, AppState>,
    selected_conversation_ids: Vec<String>,
) -> Result<AntigravityPublishResult, String> {
    let db = state.database.clone();
    let home = state.paths.antigravity_home();
    let backups = state.paths.backups.clone();
    run_blocking(move || {
        AntigravityPublisher::new(&db, Some(&home), &backups).publish(&selected_conversation_ids)
    })
    .await
}

#[tauri::command]
pub async fn get_antigravity_projects(
    state: State<'_, AppState>,
) -> Result<Vec<AntigravityProject>, String> {
    let db = state.database.clone();
    let home = state.paths.antigravity_home();
    run_blocking(move || Ok(AntigravityRecoveryService::new(&db, Some(&home)).current_projects()))
        .await
}

#[tauri::command]
pub async fn is_antigravity_client_running() -> Result<bool, String> {
    run_blocking(|| Ok(AntigravityClientController::new().is_running())).await
}

#[tauri::command]
pub async fn stop_antigravity_client() -> Result<bool, String> {
    run_blocking(|| AntigravityClientController::new().stop(15.0)).await
}

#[tauri::command]
pub async fn start_antigravity_client() -> Result<(), String> {
    run_blocking(|| AntigravityClientController::new().start(15.0)).await
}
