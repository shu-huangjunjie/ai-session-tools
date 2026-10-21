use std::process::Command;

use super::process_util::{any_running, stop_processes, wait_until};

const PROCESS_NAMES: &[&str] = &["ChatGPT.exe"];
const APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";

pub struct CodexClientController;

impl CodexClientController {
    pub fn new() -> Self {
        Self
    }

    pub fn is_running(&self) -> bool {
        any_running(PROCESS_NAMES)
    }

    pub fn stop(&self, timeout_s: f64) -> Result<bool, String> {
        stop_processes(PROCESS_NAMES, timeout_s, "Codex/ChatGPT 客户端")
    }

    pub fn start(&self, timeout_s: f64) -> Result<(), String> {
        Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{}", APP_ID))
            .spawn()
            .map_err(|e| format!("启动 Codex/ChatGPT 失败：{}", e))?;
        wait_until(timeout_s, || self.is_running());
        Ok(())
    }
}
