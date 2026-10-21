use std::path::PathBuf;
use std::process::Command;

use super::process_util::{any_running, stop_processes, wait_until};

const PROCESS_NAMES: &[&str] = &["Antigravity.exe", "Antigravity IDE.exe"];

pub struct AntigravityClientController;

impl AntigravityClientController {
    pub fn new() -> Self {
        Self
    }

    pub fn is_running(&self) -> bool {
        any_running(PROCESS_NAMES)
    }

    pub fn stop(&self, timeout_s: f64) -> Result<bool, String> {
        stop_processes(PROCESS_NAMES, timeout_s, "Antigravity 客户端")
    }

    pub fn start(&self, timeout_s: f64) -> Result<(), String> {
        let mut possible_paths: Vec<PathBuf> = Vec::new();
        if let Some(local_app) = dirs::data_local_dir() {
            let programs = local_app.join("Programs");
            possible_paths.push(programs.join("Antigravity IDE").join("Antigravity IDE.exe"));
            possible_paths.push(programs.join("antigravity").join("Antigravity.exe"));
            possible_paths.push(programs.join("Antigravity").join("Antigravity.exe"));
        }
        if let Some(appdata) = dirs::config_dir() {
            let programs = appdata.join("Programs");
            possible_paths.push(programs.join("antigravity").join("Antigravity.exe"));
            possible_paths.push(programs.join("Antigravity").join("Antigravity.exe"));
        }

        // Several installs can coexist after the IDE was renamed; start the newest.
        let exe_path = possible_paths
            .into_iter()
            .filter(|p| p.is_file())
            .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
            .ok_or_else(|| "未找到 Antigravity 安装目录，请先安装 Antigravity".to_string())?;

        Command::new(&exe_path)
            .spawn()
            .map_err(|e| format!("启动 Antigravity 失败：{}", e))?;
        wait_until(timeout_s, || self.is_running());
        Ok(())
    }
}
