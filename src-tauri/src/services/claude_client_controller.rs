use std::os::windows::process::CommandExt;
use std::process::Command;

use super::process_util::{any_running, stop_processes, wait_until};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const PROCESS_NAMES: &[&str] = &["claude.exe"];

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
        let local_app = dirs::data_local_dir()
            .map(|d| d.join("Programs").join("Claude").join("Claude.exe"));

        let res = if let Some(p) = local_app.filter(|p| p.is_file()) {
            Command::new(p).spawn()
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
