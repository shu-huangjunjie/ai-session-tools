use std::path::PathBuf;
use tauri::State;

use super::run_blocking;
use crate::importers::{ImportPreview, ImportResult, ImportService};
use crate::AppState;

fn import_path(file_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(crate::util::normalize_windows_path(file_path));
    if !path.is_file() {
        return Err(format!("文件不存在：{}", path.display()));
    }
    Ok(path)
}

#[tauri::command]
pub async fn preview_import(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<ImportPreview, String> {
    let db = state.database.clone();
    run_blocking(move || ImportService::new(&db).preview(&import_path(&file_path)?)).await
}

#[tauri::command]
pub async fn execute_import(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<ImportResult, String> {
    let db = state.database.clone();
    run_blocking(move || ImportService::new(&db).import_file(&import_path(&file_path)?)).await
}
