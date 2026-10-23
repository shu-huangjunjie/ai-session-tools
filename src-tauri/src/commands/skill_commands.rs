use tauri::State;

use super::run_blocking;
use crate::services::{CcSwitchState, SkillClient, SkillDeleteOutcome, SkillLocation, SkillLocationKind, SkillManager};
use crate::AppState;

/// `libraries` 是用户登记的技能库文件夹；写操作只接受这些目录和客户端技能目录中的技能。
fn manager(state: &AppState, libraries: &[String]) -> SkillManager {
    SkillManager::new(&state.paths.claude_home(), &state.paths.codex_home(), libraries)
}

#[tauri::command]
pub async fn list_skills(
    state: State<'_, AppState>,
    libraries: Vec<String>,
) -> Result<Vec<SkillLocation>, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || {
        let mut locations = m.list();
        // 标注 cc-switch 管理的技能：它重新同步时会按自己的记录改回这些技能。
        let cc_switch = CcSwitchState::detect();
        for location in &mut locations {
            for skill in &mut location.skills {
                skill.cc_switch = cc_switch.skill(&skill.folder_name).and_then(|apps| match location.kind {
                    SkillLocationKind::Claude => Some(apps.claude),
                    SkillLocationKind::Codex => Some(apps.codex),
                    SkillLocationKind::Library => None,
                });
            }
        }
        Ok(locations)
    })
    .await
}

#[tauri::command]
pub async fn read_skill_file(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    path: String,
) -> Result<String, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || m.read_skill_file(&path)).await
}

#[tauri::command]
pub async fn create_skill(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    target: String,
    folder_name: String,
    description: String,
    mount_to: Vec<SkillClient>,
) -> Result<String, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || {
        let dir = m.create(&target, &folder_name, &description)?;
        let path = dir.to_string_lossy().to_string();
        for client in mount_to {
            m.mount(&path, client)?;
        }
        Ok(path)
    })
    .await
}

#[tauri::command]
pub async fn set_skill_enabled(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    path: String,
    enabled: bool,
) -> Result<String, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || Ok(m.set_enabled(&path, enabled)?.to_string_lossy().to_string())).await
}

#[tauri::command]
pub async fn mount_skill(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    path: String,
    client: SkillClient,
) -> Result<String, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || Ok(m.mount(&path, client)?.to_string_lossy().to_string())).await
}

/// 把客户端中的技能以目录联接同步到另一个客户端。
#[tauri::command]
pub async fn link_skill_to_client(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    path: String,
    client: SkillClient,
) -> Result<String, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || Ok(m.link_to_client(&path, client)?.to_string_lossy().to_string())).await
}

#[tauri::command]
pub async fn delete_skill(
    state: State<'_, AppState>,
    libraries: Vec<String>,
    path: String,
) -> Result<SkillDeleteOutcome, String> {
    let m = manager(&state, &libraries);
    run_blocking(move || m.delete(&path)).await
}
