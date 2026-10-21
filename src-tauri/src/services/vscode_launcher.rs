use std::env;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct VSCodeLauncher;

impl VSCodeLauncher {
    pub fn new() -> Self {
        Self
    }

    pub fn find_executable(&self) -> Option<PathBuf> {
        // 1. 检查环境变量 PATH 中是否有 code.cmd 或 Code.exe
        if let Ok(path_var) = env::var("PATH") {
            for dir in env::split_paths(&path_var) {
                let code_exe = dir.join("Code.exe");
                if code_exe.is_file() {
                    return Some(code_exe);
                }
                let code_cmd = dir.join("code.cmd");
                if code_cmd.is_file() {
                    let parent_code = dir.parent().unwrap_or(&dir).join("Code.exe");
                    if parent_code.is_file() {
                        return Some(parent_code);
                    }
                    return Some(code_cmd);
                }
            }
        }

        // 2. 检查常见 Windows 安装目录
        let mut candidates = Vec::new();
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data);
            candidates.push(base.join("Programs").join("Microsoft VS Code").join("Code.exe"));
            candidates.push(base.join("Microsoft VS Code").join("Code.exe"));
        }
        if let Ok(prog_files) = env::var("ProgramFiles") {
            candidates.push(PathBuf::from(prog_files).join("Microsoft VS Code").join("Code.exe"));
        }
        if let Ok(prog_files_x86) = env::var("ProgramFiles(x86)") {
            candidates.push(PathBuf::from(prog_files_x86).join("Microsoft VS Code").join("Code.exe"));
        }

        for path in candidates {
            if path.is_file() {
                return Some(path);
            }
        }

        None
    }

    pub fn open_project(&self, project_path: &Path) -> Result<PathBuf, String> {
        let canonical_proj = if let Ok(canon) = project_path.canonicalize() {
            canon
        } else {
            project_path.to_path_buf()
        };

        if !canonical_proj.is_dir() {
            return Err(format!("项目目录不存在：{:?}", project_path));
        }

        // canonicalize adds a verbatim prefix on Windows. External applications
        // need the ordinary path so Explorer integration keeps working.
        let launch_path = crate::util::normalize_windows_path(&canonical_proj.to_string_lossy());

        let exe = self.find_executable()
            .ok_or_else(|| "未找到 VS Code。请先安装 VS Code，或把 code 命令加入 PATH。".to_string())?;

        let is_cmd = exe.extension().and_then(|s| s.to_str()).map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")).unwrap_or(false);

        let mut cmd = if is_cmd {
            let mut c = Command::new("cmd.exe");
            c.args(["/d", "/c", exe.to_str().unwrap(), "--new-window", &launch_path]);
            c
        } else {
            let mut c = Command::new(&exe);
            c.args(["--new-window", &launch_path]);
            c
        };

        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.spawn() {
            Ok(_) => Ok(exe),
            Err(e) => Err(format!("拉起 VS Code 失败：{}", e)),
        }
    }
}
