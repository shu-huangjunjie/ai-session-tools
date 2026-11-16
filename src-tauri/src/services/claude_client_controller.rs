use std::os::windows::process::CommandExt;
use std::process::Command;

use super::process_util::{any_running, stop_processes, wait_until};
use super::quick_launch::{find_msix_package_dir_name, is_executable_or_alias};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const PROCESS_NAMES: &[&str] = &["claude.exe", "claude-desktop.exe"];

pub struct ClaudeClientController;

impl ClaudeClientController {
    pub fn new() -> Self {
        Self
    }

    /// Also true while a Claude Code CLI session runs, because it uses the same image name.
    pub fn is_running(&self) -> bool {
        any_running(PROCESS_NAMES)
    }

    pub fn stop(&self, timeout_s: f64) -> Result<bool, String> {
        stop_processes(PROCESS_NAMES, timeout_s, "Claude 进程")
    }

    pub fn start(&self, timeout_s: f64) -> Result<(), String> {
        let exe_path = dirs::data_local_dir().and_then(|d| {
            [
                d.join("Programs").join("Claude").join("Claude.exe"),
                d.join("AnthropicClaude").join("claude.exe"),
            ]
            .into_iter()
            .find(|p| is_executable_or_alias(p))
        });

        let res = if let Some(p) = exe_path {
            Command::new(p).spawn()
        } else if let Some(pkg) = find_msix_package_dir_name("Claude_") {
            Command::new("explorer.exe")
                .arg(format!("shell:AppsFolder\\{}!Claude", pkg))
                .spawn()
        } else {
            Command::new("cmd")
                .args(["/c", "start", "", "claude"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
        };
        res.map_err(|e| format!("启动 Claude 失败：{}", e))?;
        wait_until(timeout_s, || self.is_running());
        Ok(())
    }
}
