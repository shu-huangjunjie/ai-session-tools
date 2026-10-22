pub mod antigravity_commands;
pub mod claude_commands;
pub mod codex_commands;
pub mod conversation_commands;
pub mod import_commands;
pub mod skill_commands;
pub mod system_commands;

pub use antigravity_commands::*;
pub use claude_commands::*;
pub use codex_commands::*;
pub use conversation_commands::*;
pub use import_commands::*;
pub use skill_commands::*;
pub use system_commands::*;

/// Runs blocking work (SQLite, file scans, child processes) on the blocking thread
/// pool. Synchronous Tauri commands run on the UI thread and freeze the window.
pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|e| format!("后台任务异常终止：{}", e))?
}
