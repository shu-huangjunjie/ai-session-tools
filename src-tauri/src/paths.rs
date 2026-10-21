use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub backups: PathBuf,
    pub logs: PathBuf,
    pub import_reports: PathBuf,
}

impl AppPaths {
    /// 自动发现并解析数据路径。
    /// 策略（便携优先）：
    /// 1. 优先检查环境变量 `CONTEXT_VAULT_ROOT`
    /// 2. 检查程序当前运行目录同级是否存在 `./data`（便携免安装模式）
    /// 3. 回退到标准系统的 `%LOCALAPPDATA%\ContextRestoration`
    /// 全部路径均在此处标准化为可移植路径，绝不硬编码绝对路径。
    pub fn discover(custom_root: Option<&Path>) -> Result<Self, std::io::Error> {
        let root = if let Some(r) = custom_root {
            r.to_path_buf()
        } else if let Ok(env_root) = env::var("CONTEXT_VAULT_ROOT") {
            PathBuf::from(env_root)
        } else if let Ok(current_exe) = env::current_exe() {
            let exe_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
            let local_data = exe_dir.join("data");
            if local_data.is_dir() {
                exe_dir.to_path_buf()
            } else {
                Self::default_system_root()
            }
        } else {
            Self::default_system_root()
        };

        let paths = Self {
            data: root.join("data"),
            database: root.join("data").join("conversations.db"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            import_reports: root.join("imports").join("reports"),
            root,
        };

        for dir in &[
            &paths.root,
            &paths.data,
            &paths.backups,
            &paths.logs,
            &paths.import_reports,
        ] {
            fs::create_dir_all(dir)?;
        }

        Ok(paths)
    }

    fn default_system_root() -> PathBuf {
        dirs::data_local_dir()
            .map(|d| d.join("ContextRestoration"))
            .unwrap_or_else(|| PathBuf::from("./context_restoration_data"))
    }

    /// 自动发现 Antigravity 数据根目录：
    /// 1. 便携模式：检查同级 `./data/antigravity`
    /// 2. 环境变量：检查 `ANTIGRAVITY_HOME`
    /// 3. 标准系统路径：检查用户主目录下的 `~/.gemini/antigravity`
    pub fn antigravity_home(&self) -> PathBuf {
        let portable = self.root.join("data").join("antigravity");
        if portable.is_dir() {
            return portable;
        }
        if let Ok(env_home) = env::var("ANTIGRAVITY_HOME") {
            let p = PathBuf::from(env_home);
            if p.is_dir() {
                return p;
            }
        }
        dirs::home_dir()
            .map(|h| h.join(".gemini").join("antigravity"))
            .unwrap_or_else(|| PathBuf::from(".gemini").join("antigravity"))
    }

    /// 自动发现 Claude 数据根目录：
    /// 1. 便携模式：检查同级 `./data/claude`
    /// 2. 环境变量：检查 `CLAUDE_HOME` 或 `CLAUDE_CONFIG_DIR`
    /// 3. 标准系统路径：检查用户主目录下的 `~/.claude`
    pub fn claude_home(&self) -> PathBuf {
        let portable = self.root.join("data").join("claude");
        if portable.is_dir() {
            return portable;
        }
        if let Ok(env_home) = env::var("CLAUDE_HOME") {
            let p = PathBuf::from(env_home);
            if p.is_dir() {
                return p;
            }
        }
        if let Ok(env_home) = env::var("CLAUDE_CONFIG_DIR") {
            let p = PathBuf::from(env_home);
            if p.is_dir() {
                return p;
            }
        }
        dirs::home_dir()
            .map(|h| h.join(".claude"))
            .unwrap_or_else(|| PathBuf::from(".claude"))
    }
}
