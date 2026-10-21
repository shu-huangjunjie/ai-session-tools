use tauri::State;

use super::run_blocking;
use crate::domain::{CanonicalMessage, Conversation, ConversationSummary};
use crate::persistence::{Repository, ResetScope, ResetSummary};
use crate::AppState;

#[tauri::command]
pub async fn list_folder_pins(state: State<'_, AppState>) -> Result<Vec<crate::persistence::repository::FolderPin>, String> {
    let db = state.database.clone();
    run_blocking(move || Repository::new(&db).list_folder_pins().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn set_folder_pinned(state: State<'_, AppState>, source: String, path: String, pinned: bool) -> Result<(), String> {
    let db = state.database.clone();
    run_blocking(move || Repository::new(&db).set_folder_pinned(&source, &path, pinned).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn list_conversations(
    state: State<'_, AppState>,
    archived: bool,
    limit: Option<i64>,
) -> Result<Vec<ConversationSummary>, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .list_conversation_summaries(archived, limit)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_conversation(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<Conversation>, String> {
    let db = state.database.clone();
    run_blocking(move || Repository::new(&db).get_conversation(&id).map_err(|e| e.to_string()))
        .await
}

#[tauri::command]
pub async fn rename_conversation(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<bool, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .rename_conversation(&id, &title)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_archived(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<bool, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .set_archived(&id, archived)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_pinned(
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<bool, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .set_pinned(&id, pinned)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_conversation(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .delete_conversation(&id)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_conversations(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<usize, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .delete_conversations(&ids)
            .map_err(|e| e.to_string())
    })
    .await
}

#[derive(serde::Serialize)]
pub struct ResetResult {
    #[serde(flatten)]
    pub summary: ResetSummary,
    pub backup_path: Option<String>,
}

/// Clears conversations so they can be scanned and imported again. A real reset
/// first backs up the database and stops if that fails.
#[tauri::command]
pub async fn reset_conversations(
    state: State<'_, AppState>,
    scope: String,
    clear_exclusions: bool,
    dry_run: bool,
) -> Result<ResetResult, String> {
    let scope = ResetScope::parse(&scope).ok_or_else(|| format!("未知的清空范围：{}", scope))?;
    let db = state.database.clone();
    run_blocking(move || {
        let repo = Repository::new(&db);
        if dry_run {
            let summary = repo
                .reset_conversations(scope, clear_exclusions, true)
                .map_err(|e| e.to_string())?;
            return Ok(ResetResult { summary, backup_path: None });
        }
        let backup = db
            .backup(None)
            .map_err(|e| format!("备份失败，未清空任何数据：{}", e))?;
        let summary = repo
            .reset_conversations(scope, clear_exclusions, false)
            .map_err(|e| e.to_string())?;
        Ok(ResetResult {
            summary,
            backup_path: Some(backup.to_string_lossy().to_string()),
        })
    })
    .await
}

#[tauri::command]
pub async fn get_messages(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<CanonicalMessage>, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .load_active_path(&conversation_id)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn search_conversations(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
    include_archived: Option<bool>,
) -> Result<Vec<ConversationSummary>, String> {
    let db = state.database.clone();
    run_blocking(move || {
        Repository::new(&db)
            .search_conversations_with_archived(
                &query,
                limit.unwrap_or(200),
                include_archived.unwrap_or(false),
            )
            .map_err(|e| e.to_string())
    })
    .await
}
