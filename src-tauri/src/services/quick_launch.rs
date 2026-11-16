use std::collections::{BTreeMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::services::{account_store, process_util, VSCodeLauncher};
use crate::util::{normalize_windows_path, percent_encode_component};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x00000010;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickLaunchCategory {
    AiApp,
    AiCli,
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickLaunchTool {
    pub id: String,
    pub name: String,
    pub category: QuickLaunchCategory,
    pub available: bool,
    pub running: bool,
    pub detected_path: Option<String>,
    pub custom_path: Option<String>,
    pub supports_folder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct QuickLaunchConfig {
    #[serde(default)]
    custom_paths: BTreeMap<String, String>,
}

struct ToolSpec {
    id: &'static str,
    name: &'static str,
    category: QuickLaunchCategory,
    supports_folder: bool,
    process_names: &'static [&'static str],
    detect: fn() -> Option<String>,
}

const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        id: "codex_app",
        name: "Codex",
        category: QuickLaunchCategory::AiApp,
        supports_folder: true,
        process_names: &["ChatGPT.exe", "Codex.exe"],
        detect: detect_codex_app,
    },
    ToolSpec {
        id: "claude_app",
        name: "Claude",
        category: QuickLaunchCategory::AiApp,
        supports_folder: true,
        process_names: &["claude.exe", "claude-desktop.exe"],
        detect: detect_claude_app,
    },
    ToolSpec {
        id: "antigravity_app",
        name: "Antigravity",
        category: QuickLaunchCategory::AiApp,
        supports_folder: true,
        process_names: &["Antigravity.exe", "Antigravity IDE.exe"],
        detect: detect_antigravity_app,
    },
    ToolSpec {
        id: "codex_cli",
        name: "Codex CLI",
        category: QuickLaunchCategory::AiCli,
        supports_folder: true,
        process_names: &["codex.exe"],
        detect: || find_codex_cli_executable().map(path_to_string),
    },
    ToolSpec {
        id: "claude_cli",
        name: "Claude Code",
        category: QuickLaunchCategory::AiCli,
        supports_folder: true,
        process_names: &["claude.exe"],
        detect: || find_claude_cli_executable().map(path_to_string),
    },
    ToolSpec {
        id: "antigravity_cli",
        name: "Antigravity CLI",
        category: QuickLaunchCategory::AiCli,
        supports_folder: true,
        process_names: &["agy.exe"],
        detect: || find_agy_cli_executable().map(path_to_string),
    },
    ToolSpec {
        id: "vscode",
        name: "VS Code",
        category: QuickLaunchCategory::Editor,
        supports_folder: true,
        process_names: &["Code.exe"],
        detect: || VSCodeLauncher::new().find_executable().map(path_to_string),
    },
    ToolSpec {
        id: "cursor",
        name: "Cursor",
        category: QuickLaunchCategory::Editor,
        supports_folder: true,
        process_names: &["Cursor.exe"],
        detect: detect_cursor,
    },
    ToolSpec {
        id: "windsurf",
        name: "Windsurf",
        category: QuickLaunchCategory::Editor,
        supports_folder: true,
        process_names: &["Windsurf.exe"],
        detect: detect_windsurf,
    },
    ToolSpec {
        id: "trae",
        name: "Trae",
        category: QuickLaunchCategory::Editor,
        supports_folder: true,
        process_names: &["Trae.exe", "Trae CN.exe"],
        detect: detect_trae,
    },
    ToolSpec {
        id: "windows_terminal",
        name: "Windows Terminal",
        category: QuickLaunchCategory::Terminal,
        supports_folder: true,
        process_names: &["WindowsTerminal.exe"],
        detect: detect_windows_terminal,
    },
    ToolSpec {
        id: "powershell",
        name: "PowerShell",
        category: QuickLaunchCategory::Terminal,
        supports_folder: true,
        process_names: &["pwsh.exe", "powershell.exe"],
        detect: detect_powershell,
    },
    ToolSpec {
        id: "git_bash",
        name: "Git Bash",
        category: QuickLaunchCategory::Terminal,
        supports_folder: true,
        process_names: &["mintty.exe", "git-bash.exe"],
        detect: detect_git_bash,
    },
];

pub struct QuickLaunchService {
    config_path: PathBuf,
}

impl QuickLaunchService {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            config_path: data_dir.join("quick-launch.json"),
        }
    }

    pub fn list_tools(&self) -> Result<Vec<QuickLaunchTool>, String> {
        let cfg = self.load_config();
        let running = process_util::running_image_names();
        Ok(TOOL_SPECS
            .iter()
            .map(|spec| build_tool(spec, &cfg, &running))
            .collect())
    }

    pub fn set_custom_path(
        &self,
        tool_id: &str,
        custom_path: Option<String>,
    ) -> Result<Vec<QuickLaunchTool>, String> {
        let _spec = find_spec(tool_id)?;
        let mut cfg = self.load_config();
        match custom_path {
            Some(raw) if !raw.trim().is_empty() => {
                let normalized = normalize_windows_path(raw.trim());
                if !normalized.starts_with("shell:AppsFolder\\")
                    && !is_executable_or_alias(Path::new(&normalized))
                {
                    return Err(format!("指定的可执行文件不存在：{}", normalized));
                }
                cfg.custom_paths.insert(tool_id.to_string(), normalized);
            }
            _ => {
                cfg.custom_paths.remove(tool_id);
            }
        }
        self.save_config(&cfg)?;
        self.list_tools()
    }

    /// 校验快捷启动工具目标与工作目录是否有效；成功时返回解析后的可执行目标路径。
    pub fn validate_launch_request(
        &self,
        tool_id: &str,
        folder_path: Option<&str>,
    ) -> Result<String, String> {
        let spec = find_spec(tool_id)?;
        let cfg = self.load_config();
        let custom = cfg
            .custom_paths
            .get(tool_id)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let target = if let Some(path) = custom {
            if !path.starts_with("shell:AppsFolder\\") && !is_executable_or_alias(Path::new(&path)) {
                return Err(format!("{} 自定义路径无效或不存在：{}", spec.name, path));
            }
            path
        } else {
            (spec.detect)().ok_or_else(|| {
                format!(
                    "未检测到 {}，请确认已安装或在配置中手动指定可执行路径",
                    spec.name
                )
            })?
        };
        let _ = resolve_folder(folder_path)?;
        Ok(target)
    }

    pub fn launch_tool(&self, tool_id: &str, folder_path: Option<&str>) -> Result<(), String> {
        self.launch_tool_with_options(tool_id, folder_path, None, None)
    }

    /// 通过可选终端类型与自定义 CLI 参数启动对应桌面应用、CLI 或终端。
    pub fn launch_tool_with_options(
        &self,
        tool_id: &str,
        folder_path: Option<&str>,
        terminal: Option<&str>,
        custom_command: Option<&str>,
    ) -> Result<(), String> {
        self.launch_tool_with_options_and_file(tool_id, folder_path, terminal, custom_command, None)
    }

    /// 启动工具；针对编辑器额外支持同时打开指定交接文件（如 `.context-vault/handoff-*.md`）。
    pub fn launch_tool_with_options_and_file(
        &self,
        tool_id: &str,
        folder_path: Option<&str>,
        terminal: Option<&str>,
        custom_command: Option<&str>,
        open_file: Option<&Path>,
    ) -> Result<(), String> {
        let spec = find_spec(tool_id)?;
        let cfg = self.load_config();
        let target = self.validate_launch_request(tool_id, folder_path)?;
        let resolved_dir = resolve_folder(folder_path)?;

        if target.starts_with("shell:AppsFolder\\") {
            if let Some(dir) = resolved_dir.as_deref() {
                let dir_str = dir.to_string_lossy().to_string();
                if spec.id == "codex_app" {
                    let uri = format!(
                        "codex://threads/new?path={}",
                        percent_encode_component(&dir_str)
                    );
                    return open_shell_uri(&uri, spec.name);
                }
                if spec.id == "claude_app" {
                    let uri = format!(
                        "claude://code/new?folder={}",
                        percent_encode_component(&dir_str)
                    );
                    return open_shell_uri(&uri, spec.name);
                }
            }
            Command::new("explorer.exe")
                .arg(&target)
                .spawn()
                .map_err(|e| format!("启动 {} 失败：{}", spec.name, e))?;
            return Ok(());
        }

        match spec.category {
            QuickLaunchCategory::AiCli => {
                let dir = resolved_dir.or_else(dirs::home_dir);
                let custom_wt = cfg.custom_paths.get("windows_terminal").map(String::as_str);
                let custom_git_bash = cfg.custom_paths.get("git_bash").map(String::as_str);
                spawn_cli_in_terminal(
                    &target,
                    dir.as_deref(),
                    spec.id == "claude_cli",
                    spec.name,
                    terminal,
                    custom_command,
                    custom_wt,
                    custom_git_bash,
                )
            }
            QuickLaunchCategory::Terminal => {
                let dir = resolved_dir.or_else(dirs::home_dir);
                spawn_terminal(spec.id, &target, dir.as_deref(), spec.name)
            }
            QuickLaunchCategory::Editor | QuickLaunchCategory::AiApp => spawn_gui_app(
                spec.id,
                &target,
                resolved_dir.as_deref(),
                open_file.filter(|_| spec.category == QuickLaunchCategory::Editor),
                spec.name,
            ),
        }
    }

    fn load_config(&self) -> QuickLaunchConfig {
        std::fs::read_to_string(&self.config_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<QuickLaunchConfig>(&raw).ok())
            .unwrap_or_default()
    }

    fn save_config(&self, cfg: &QuickLaunchConfig) -> Result<(), String> {
        let json = serde_json::to_string_pretty(cfg)
            .map_err(|e| format!("序列化快速启动配置失败：{}", e))?;
        account_store::atomic_write(&self.config_path, json.as_bytes())
    }
}

fn find_spec(tool_id: &str) -> Result<&'static ToolSpec, String> {
    TOOL_SPECS
        .iter()
        .find(|s| s.id == tool_id)
        .ok_or_else(|| format!("未知的快速启动工具：{}", tool_id))
}

fn build_tool(
    spec: &ToolSpec,
    cfg: &QuickLaunchConfig,
    running_images: &HashSet<String>,
) -> QuickLaunchTool {
    let custom_path = cfg.custom_paths.get(spec.id).cloned();
    let detected_path = (spec.detect)();
    let custom_valid = custom_path.as_deref().is_some_and(|p| {
        p.starts_with("shell:AppsFolder\\") || is_executable_or_alias(Path::new(p))
    });
    let available = custom_valid || detected_path.is_some();
    let running = spec
        .process_names
        .iter()
        .any(|name| running_images.contains(&name.to_ascii_lowercase()));

    QuickLaunchTool {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        category: spec.category,
        available,
        running,
        detected_path,
        custom_path,
        supports_folder: spec.supports_folder,
    }
}

fn resolve_folder(folder_path: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(raw) = folder_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let normalized = normalize_windows_path(raw);
    let path = PathBuf::from(&normalized);
    if !path.is_dir() {
        return Err(format!("目标工作目录不存在：{}", normalized));
    }
    Ok(Some(path))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 将 PowerShell 脚本编码为 UTF-16LE 标准 Base64，供 `-EncodedCommand` 安全传参使用。
pub fn encode_utf16le_base64(script: &str) -> String {
    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64_encode(&bytes)
}

fn split_custom_cli_args(target: &str, custom_command: Option<&str>) -> Option<String> {
    let trimmed = custom_command?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(target);
    let mut prefixes: Vec<String> = Vec::new();
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        prefixes.push(name.to_string());
    }
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if !prefixes.iter().any(|p| p.eq_ignore_ascii_case(stem)) {
            prefixes.push(stem.to_string());
        }
    }
    prefixes.sort_by(|a, b| b.len().cmp(&a.len()));

    for prefix in prefixes {
        if !trimmed.is_char_boundary(prefix.len()) {
            continue;
        }
        if trimmed[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            let rest = &trimmed[prefix.len()..];
            if rest.is_empty() {
                return None;
            }
            if rest.starts_with(char::is_whitespace) {
                let args = rest.trim();
                return if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
            }
        }
    }
    Some(trimmed.to_string())
}

#[cfg(windows)]
fn build_powershell_cli_script(
    target: &str,
    dir: Option<&Path>,
    strip_claude_provider: bool,
    custom_command: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if strip_claude_provider {
        for key in crate::services::claude_accounts::PROVIDER_KEYS {
            parts.push(format!(
                "Remove-Item -Path 'Env:{}' -ErrorAction SilentlyContinue",
                key
            ));
        }
    }
    if let Some(d) = dir {
        let escaped_dir = d.to_string_lossy().replace('\'', "''");
        parts.push(format!("Set-Location -LiteralPath '{}'", escaped_dir));
    }
    let escaped_target = target.replace('\'', "''");
    if let Some(args) = split_custom_cli_args(target, custom_command) {
        parts.push(format!("& '{}' {}", escaped_target, args));
    } else {
        parts.push(format!("& '{}'", escaped_target));
    }
    parts.join("; ")
}

#[cfg(windows)]
fn detect_pwsh_only() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(pf) = env::var("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("PowerShell").join("7").join("pwsh.exe"));
    }
    first_existing(candidates).or_else(|| find_in_path(&["pwsh.exe"]).map(path_to_string))
}

#[cfg(windows)]
fn detect_windows_powershell() -> String {
    if let Ok(sysroot) = env::var("SystemRoot") {
        let win_ps = PathBuf::from(sysroot)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if is_executable_or_alias(&win_ps) {
            return path_to_string(win_ps);
        }
    }
    find_in_path(&["powershell.exe"])
        .map(path_to_string)
        .unwrap_or_else(|| "powershell.exe".to_string())
}

#[allow(clippy::too_many_arguments)]
fn spawn_cli_in_terminal(
    target: &str,
    dir: Option<&Path>,
    strip_claude_provider: bool,
    label: &str,
    terminal: Option<&str>,
    custom_command: Option<&str>,
    custom_wt_path: Option<&str>,
    custom_git_bash_path: Option<&str>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let term_kind = terminal
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("system")
            .to_ascii_lowercase();

        let apply_env = |cmd: &mut Command| {
            if strip_claude_provider {
                for key in crate::services::claude_accounts::PROVIDER_KEYS {
                    cmd.env_remove(key);
                }
            }
        };

        match term_kind.as_str() {
            "wt" => {
                let wt = custom_wt_path
                    .map(str::trim)
                    .filter(|p| !p.is_empty() && is_executable_or_alias(Path::new(p)))
                    .map(ToString::to_string)
                    .or_else(detect_windows_terminal)
                    .ok_or_else(|| {
                        "未检测到 Windows Terminal (wt.exe)，请在快速启动设置中指定路径".to_string()
                    })?;
                let shell = detect_powershell().unwrap_or_else(|| "powershell.exe".to_string());
                let script =
                    build_powershell_cli_script(target, dir, strip_claude_provider, custom_command);
                let encoded = encode_utf16le_base64(&script);
                let mut cmd = Command::new(&wt);
                if let Some(d) = dir {
                    cmd.arg("-d").arg(d);
                    cmd.current_dir(d);
                }
                cmd.args([shell.as_str(), "-NoExit", "-EncodedCommand", &encoded]);
                apply_env(&mut cmd);
                cmd.spawn()
                    .map_err(|e| format!("在 Windows Terminal 中启动 {} 失败：{}", label, e))?;
                Ok(())
            }
            "pwsh" => {
                let pwsh = detect_pwsh_only().ok_or_else(|| {
                    "未检测到 PowerShell 7 (pwsh.exe)，请确认已安装或切换其他终端".to_string()
                })?;
                let script =
                    build_powershell_cli_script(target, dir, strip_claude_provider, custom_command);
                let encoded = encode_utf16le_base64(&script);
                let mut cmd = Command::new(&pwsh);
                cmd.args(["-NoExit", "-EncodedCommand", &encoded])
                    .creation_flags(CREATE_NEW_CONSOLE);
                if let Some(d) = dir {
                    cmd.current_dir(d);
                }
                apply_env(&mut cmd);
                cmd.spawn()
                    .map_err(|e| format!("在 PowerShell 7 中启动 {} 失败：{}", label, e))?;
                Ok(())
            }
            "cmd" => {
                let invocation = if let Some(args) = split_custom_cli_args(target, custom_command) {
                    format!("call \"{}\" {}", target, args)
                } else {
                    format!("call \"{}\"", target)
                };
                let mut cmd = Command::new("cmd.exe");
                cmd.creation_flags(CREATE_NEW_CONSOLE);
                if let Some(d) = dir {
                    cmd.current_dir(d);
                }
                apply_env(&mut cmd);
                cmd.raw_arg(format!("/k \"{}\"", invocation));
                cmd.spawn()
                    .map_err(|e| format!("在 CMD 中启动 {} 失败：{}", label, e))?;
                Ok(())
            }
            "git_bash" => {
                let git_bash = custom_git_bash_path
                    .map(str::trim)
                    .filter(|p| !p.is_empty() && is_executable_or_alias(Path::new(p)))
                    .map(ToString::to_string)
                    .or_else(detect_git_bash)
                    .ok_or_else(|| {
                        "未检测到 Git Bash (git-bash.exe)，请在快速启动设置中指定路径".to_string()
                    })?;
                let target_posix = target.replace('\\', "/").replace('\'', "'\\''");
                let invocation = if let Some(args) = split_custom_cli_args(target, custom_command) {
                    format!("'{}' {}", target_posix, args)
                } else {
                    format!("'{}'", target_posix)
                };
                let mut parts = Vec::new();
                if strip_claude_provider {
                    for key in crate::services::claude_accounts::PROVIDER_KEYS {
                        parts.push(format!("unset {}", key));
                    }
                }
                parts.push(invocation);
                parts.push("exec bash".to_string());
                let bash_script = parts.join("; ");
                let mut cmd = Command::new(&git_bash);
                if let Some(d) = dir {
                    cmd.current_dir(d);
                    cmd.arg(format!("--cd={}", d.to_string_lossy().replace('\\', "/")));
                }
                apply_env(&mut cmd);
                cmd.args(["-c", &bash_script]);
                cmd.spawn()
                    .map_err(|e| format!("在 Git Bash 中启动 {} 失败：{}", label, e))?;
                Ok(())
            }
            _ => {
                let ps_exe = detect_windows_powershell();
                let script =
                    build_powershell_cli_script(target, dir, strip_claude_provider, custom_command);
                let encoded = encode_utf16le_base64(&script);
                let mut cmd = Command::new(&ps_exe);
                cmd.args(["-NoExit", "-EncodedCommand", &encoded])
                    .creation_flags(CREATE_NEW_CONSOLE);
                if let Some(d) = dir {
                    cmd.current_dir(d);
                }
                apply_env(&mut cmd);
                cmd.spawn()
                    .map_err(|e| format!("启动 {} 失败：{}", label, e))?;
                Ok(())
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (strip_claude_provider, terminal, custom_wt_path, custom_git_bash_path);
        let mut cmd = Command::new(target);
        if let Some(args) = split_custom_cli_args(target, custom_command) {
            for part in args.split_whitespace() {
                cmd.arg(part);
            }
        }
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        cmd.spawn()
            .map_err(|e| format!("启动 {} 失败：{}", label, e))?;
        Ok(())
    }
}

fn spawn_terminal(
    tool_id: &str,
    target: &str,
    dir: Option<&Path>,
    label: &str,
) -> Result<(), String> {
    let mut cmd = match tool_id {
        "windows_terminal" => {
            let mut c = Command::new(target);
            if let Some(d) = dir {
                c.args(["-d", &d.to_string_lossy()]);
            }
            c
        }
        "git_bash" => {
            let mut c = Command::new(target);
            if let Some(d) = dir {
                c.arg(format!("--cd={}", d.to_string_lossy()));
            }
            c
        }
        _ => {
            let mut c = Command::new(target);
            c.arg("-NoExit");
            if let Some(d) = dir {
                c.current_dir(d);
            }
            #[cfg(windows)]
            c.creation_flags(CREATE_NEW_CONSOLE);
            c
        }
    };

    cmd.spawn()
        .map_err(|e| format!("启动 {} 失败：{}", label, e))?;
    Ok(())
}

/// 通过系统协议处理器打开深度链接（如 `codex://...`、`claude://...`），避免 cmd metacharacters 截断参数。
pub fn open_shell_uri(uri: &str, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let ps_exe = detect_windows_powershell();
        let escaped_uri = uri.replace('\'', "''");
        let script = format!("Start-Process -FilePath '{}'", escaped_uri);
        let encoded = encode_utf16le_base64(&script);
        Command::new(&ps_exe)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-EncodedCommand",
                &encoded,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("打开 {} 深度链接失败：{}", label, e))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Command::new("xdg-open")
            .arg(uri)
            .spawn()
            .map_err(|e| format!("打开 {} 深度链接失败：{}", label, e))?;
        Ok(())
    }
}

fn spawn_gui_app(
    tool_id: &str,
    target: &str,
    dir: Option<&Path>,
    open_file: Option<&Path>,
    label: &str,
) -> Result<(), String> {
    let is_script = Path::new(target)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));

    let mut args: Vec<String> = Vec::new();
    if let Some(d) = dir {
        let dir_str = d.to_string_lossy().to_string();
        if tool_id == "vscode" {
            args.push("--new-window".to_string());
        }
        args.push(dir_str);
    }
    if let Some(file) = open_file {
        args.push(file.to_string_lossy().to_string());
    }

    let mut cmd = if is_script {
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/c", target]);
        c.args(&args);
        #[cfg(windows)]
        c.creation_flags(CREATE_NO_WINDOW);
        c
    } else {
        let mut c = Command::new(target);
        c.args(&args);
        c
    };

    cmd.spawn()
        .map_err(|e| format!("启动 {} 失败：{}", label, e))?;
    Ok(())
}

/// Checks regular files AND Windows Store App Execution Aliases (`IO_REPARSE_TAG_APPEXECLINK`
/// under `%LOCALAPPDATA%\Microsoft\WindowsApps`), where `Path::is_file()` can return false.
pub fn is_executable_or_alias(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
    if path.extension().is_none() {
        return false;
    }
    std::fs::symlink_metadata(path)
        .map(|m| !m.is_dir())
        .unwrap_or(false)
}

/// Returns the first installed MSIX package folder name in `%LOCALAPPDATA%\Packages`
/// whose directory name starts with `prefix` (case-insensitive).
pub fn find_msix_package_dir_name(prefix: &str) -> Option<String> {
    let packages = dirs::data_local_dir()?.join("Packages");
    let entries = std::fs::read_dir(packages).ok()?;
    let lower_prefix = prefix.to_ascii_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().starts_with(&lower_prefix) {
            return Some(name);
        }
    }
    None
}

pub fn is_msix_package_installed(prefix: &str) -> bool {
    find_msix_package_dir_name(prefix).is_some()
}

pub fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        for name in names {
            let candidate = dir.join(name);
            if is_executable_or_alias(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn find_codex_cli_executable() -> Option<PathBuf> {
    let newest_desktop_cli = dirs::data_local_dir()
        .map(|d| d.join("OpenAI").join("Codex").join("bin"))
        .and_then(|bin| std::fs::read_dir(bin).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("codex.exe"))
        .filter(|p| is_executable_or_alias(p))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    if newest_desktop_cli.is_some() {
        return newest_desktop_cli;
    }

    if let Some(appdata) = dirs::config_dir() {
        for name in ["codex.cmd", "codex.exe"] {
            let p = appdata.join("npm").join(name);
            if is_executable_or_alias(&p) {
                return Some(p);
            }
        }
    }

    find_in_path(&["codex.exe", "codex.cmd"])
}

pub fn find_claude_cli_executable() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        for name in ["claude.exe", "claude.cmd"] {
            let p = home.join(".local").join("bin").join(name);
            if is_executable_or_alias(&p) {
                return Some(p);
            }
        }
    }

    if let Some(appdata) = dirs::config_dir() {
        for name in ["claude.cmd", "claude.exe"] {
            let p = appdata.join("npm").join(name);
            if is_executable_or_alias(&p) {
                return Some(p);
            }
        }
    }

    if let Some(in_path) = find_in_path(&["claude.exe", "claude.cmd"]) {
        return Some(in_path);
    }

    // Claude Desktop MSIX bundles versioned claude.exe under %LOCALAPPDATA%\Claude-Data\<pkg>\claude-code\<ver>\<hash>\claude.exe.
    let claude_data = dirs::data_local_dir()?.join("Claude-Data");
    let mut bundled = Vec::new();
    for pkg in std::fs::read_dir(claude_data).ok()?.flatten() {
        let cc_dir = pkg.path().join("claude-code");
        if !cc_dir.is_dir() {
            continue;
        }
        if let Ok(versions) = std::fs::read_dir(cc_dir) {
            for ver in versions.flatten() {
                if !ver.path().is_dir() {
                    continue;
                }
                if let Ok(hashes) = std::fs::read_dir(ver.path()) {
                    for hash_dir in hashes.flatten() {
                        let exe = hash_dir.path().join("claude.exe");
                        if is_executable_or_alias(&exe) {
                            bundled.push(exe);
                        }
                    }
                }
            }
        }
    }
    bundled
        .into_iter()
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

pub fn find_agy_cli_executable() -> Option<PathBuf> {
    if let Some(local) = dirs::data_local_dir() {
        for name in ["agy.exe", "agy.cmd"] {
            let cand = local.join("agy").join("bin").join(name);
            if is_executable_or_alias(&cand) {
                return Some(cand);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        for cand in [
            home.join(".gemini").join("antigravity-cli").join("agy.exe"),
            home.join(".gemini").join("antigravity-cli").join("agy.cmd"),
            home.join(".local").join("bin").join("agy.exe"),
            home.join(".local").join("bin").join("agy.cmd"),
        ] {
            if is_executable_or_alias(&cand) {
                return Some(cand);
            }
        }
    }
    if let Some(local) = dirs::data_local_dir() {
        for dir in ["antigravity", "Antigravity", "Antigravity IDE"] {
            for name in ["agy.cmd", "agy.exe"] {
                let p = local.join("Programs").join(dir).join("bin").join(name);
                if is_executable_or_alias(&p) {
                    return Some(p);
                }
            }
        }
    }
    find_in_path(&["agy.exe", "agy.cmd"])
}

fn path_to_string(path: PathBuf) -> String {
    normalize_windows_path(&path.to_string_lossy())
}

fn first_existing(candidates: Vec<PathBuf>) -> Option<String> {
    candidates
        .into_iter()
        .find(|p| is_executable_or_alias(p))
        .map(path_to_string)
}

fn detect_codex_app() -> Option<String> {
    find_msix_package_dir_name("OpenAI.Codex_")
        .map(|pkg| format!("shell:AppsFolder\\{}!App", pkg))
}

fn detect_claude_app() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join("Claude").join("Claude.exe"));
        candidates.push(local.join("AnthropicClaude").join("claude.exe"));
    }
    if let Some(found) = first_existing(candidates) {
        return Some(found);
    }
    find_msix_package_dir_name("Claude_")
        .map(|pkg| format!("shell:AppsFolder\\{}!Claude", pkg))
}

fn detect_antigravity_app() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local_app) = dirs::data_local_dir() {
        let programs = local_app.join("Programs");
        candidates.push(programs.join("Antigravity IDE").join("Antigravity IDE.exe"));
        candidates.push(programs.join("antigravity").join("Antigravity.exe"));
        candidates.push(programs.join("Antigravity").join("Antigravity.exe"));
    }
    if let Some(appdata) = dirs::config_dir() {
        let programs = appdata.join("Programs");
        candidates.push(programs.join("antigravity").join("Antigravity.exe"));
        candidates.push(programs.join("Antigravity").join("Antigravity.exe"));
    }
    candidates
        .into_iter()
        .filter(|p| is_executable_or_alias(p))
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
        .map(path_to_string)
}

fn detect_cursor() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join("cursor").join("Cursor.exe"));
        candidates.push(local.join("Programs").join("Cursor").join("Cursor.exe"));
        candidates.push(local.join("cursor").join("Cursor.exe"));
    }
    if let Ok(pf) = env::var("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("Cursor").join("Cursor.exe"));
    }
    first_existing(candidates)
        .or_else(|| find_in_path(&["cursor.exe", "Cursor.exe", "cursor.cmd"]).map(path_to_string))
}

fn detect_windsurf() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join("Windsurf").join("Windsurf.exe"));
        candidates.push(local.join("Programs").join("windsurf").join("Windsurf.exe"));
    }
    if let Ok(pf) = env::var("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("Windsurf").join("Windsurf.exe"));
    }
    first_existing(candidates).or_else(|| {
        find_in_path(&["windsurf.exe", "Windsurf.exe", "windsurf.cmd"]).map(path_to_string)
    })
}

fn detect_trae() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join("Trae").join("Trae.exe"));
        candidates.push(local.join("Programs").join("Trae CN").join("Trae CN.exe"));
    }
    if let Ok(pf) = env::var("ProgramFiles") {
        let base = PathBuf::from(pf);
        candidates.push(base.join("Trae").join("Trae.exe"));
        candidates.push(base.join("Trae CN").join("Trae CN.exe"));
    }
    first_existing(candidates)
        .or_else(|| find_in_path(&["trae.exe", "Trae.exe", "trae-cn.exe"]).map(path_to_string))
}

fn detect_windows_terminal() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Microsoft").join("WindowsApps").join("wt.exe"));
    }
    first_existing(candidates).or_else(|| find_in_path(&["wt.exe"]).map(path_to_string))
}

fn detect_powershell() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(pf) = env::var("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("PowerShell").join("7").join("pwsh.exe"));
    }
    if let Some(found) = first_existing(candidates) {
        return Some(found);
    }
    if let Some(in_path) = find_in_path(&["pwsh.exe", "powershell.exe"]) {
        return Some(path_to_string(in_path));
    }
    if let Ok(sysroot) = env::var("SystemRoot") {
        let win_ps = PathBuf::from(sysroot)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if is_executable_or_alias(&win_ps) {
            return Some(path_to_string(win_ps));
        }
    }
    None
}

fn detect_git_bash() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(pf) = env::var("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("Git").join("git-bash.exe"));
    }
    if let Ok(pfx86) = env::var("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(pfx86).join("Git").join("git-bash.exe"));
    }
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("Programs").join("Git").join("git-bash.exe"));
    }
    if let Some(found) = first_existing(candidates) {
        return Some(found);
    }
    if let Some(git_exe) = find_in_path(&["git.exe"]) {
        for ancestor in git_exe.ancestors().skip(1).take(3) {
            let bash = ancestor.join("git-bash.exe");
            if is_executable_or_alias(&bash) {
                return Some(path_to_string(bash));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn encode_utf16le_base64_produces_valid_little_endian_payload() {
        assert_eq!(encode_utf16le_base64("a"), "YQA=");
        let encoded = encode_utf16le_base64("& 'codex'");
        assert_eq!(encoded.len() % 4, 0);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn split_custom_cli_args_normalizes_flags_and_prefixed_command() {
        assert_eq!(
            split_custom_cli_args(r"C:\bin\codex.exe", Some("--yolo")),
            Some("--yolo".to_string())
        );
        assert_eq!(
            split_custom_cli_args(r"C:\bin\codex.exe", Some("codex --yolo")),
            Some("--yolo".to_string())
        );
        assert_eq!(
            split_custom_cli_args(r"C:\bin\codex.exe", Some("codex.exe --yolo -m o3")),
            Some("--yolo -m o3".to_string())
        );
        assert_eq!(
            split_custom_cli_args(r"C:\bin\codex.exe", Some("codex")),
            None
        );
        assert_eq!(
            split_custom_cli_args(r"C:\bin\codex.exe", Some("   ")),
            None
        );
    }

    #[test]
    fn validate_launch_request_checks_custom_target_and_folder() {
        let dir = TestTempDir::new();
        let data_dir = dir.path().join("data");
        let work_dir = dir.path().join("workspace");
        fs::create_dir_all(&data_dir).expect("create data dir");
        fs::create_dir_all(&work_dir).expect("create workspace dir");

        let fake_exe = dir.path().join("codex.exe");
        fs::write(&fake_exe, b"MZ").expect("create dummy exe");

        let service = QuickLaunchService::new(&data_dir);
        service
            .set_custom_path("codex_cli", Some(fake_exe.to_string_lossy().to_string()))
            .expect("save custom path");

        let ok = service
            .validate_launch_request("codex_cli", Some(&work_dir.to_string_lossy()))
            .expect("valid target & folder should succeed");
        assert_eq!(ok, fake_exe.to_string_lossy());

        let missing_dir = dir.path().join("no-such-dir");
        assert!(service
            .validate_launch_request("codex_cli", Some(&missing_dir.to_string_lossy()))
            .is_err());

        fs::remove_file(&fake_exe).expect("remove dummy exe");
        assert!(service.validate_launch_request("codex_cli", None).is_err());
    }

    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("cv-ql-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_all_catalog_tools_and_manages_custom_paths() {
        let dir = TestTempDir::new();
        let svc = QuickLaunchService::new(dir.path());

        let tools = svc.list_tools().expect("list tools");
        assert_eq!(tools.len(), 13);
        assert!(tools.iter().any(|t| t.id == "vscode"));
        assert!(tools.iter().any(|t| t.id == "codex_cli"));

        let fake_exe = dir.path().join("my-editor.exe");
        fs::write(&fake_exe, b"stub").expect("write stub exe");

        let updated = svc
            .set_custom_path("cursor", Some(fake_exe.to_string_lossy().to_string()))
            .expect("set custom path");
        let cursor = updated.iter().find(|t| t.id == "cursor").unwrap();
        assert!(cursor.available);
        assert_eq!(
            cursor.custom_path.as_deref(),
            Some(fake_exe.to_string_lossy().as_ref())
        );

        let cleared = svc
            .set_custom_path("cursor", None)
            .expect("clear custom path");
        assert!(cleared
            .iter()
            .find(|t| t.id == "cursor")
            .unwrap()
            .custom_path
            .is_none());
    }

    #[test]
    fn rejects_missing_custom_path_unknown_tool_and_missing_folder() {
        let dir = TestTempDir::new();
        let svc = QuickLaunchService::new(dir.path());

        assert!(svc.set_custom_path("unknown_tool", None).is_err());
        assert!(svc
            .set_custom_path("vscode", Some(dir.path().join("missing.exe").to_string_lossy().to_string()))
            .is_err());
        assert!(svc.launch_tool("unknown_tool", None).is_err());
        assert!(svc
            .launch_tool("powershell", Some(&dir.path().join("no-such-dir").to_string_lossy()))
            .is_err());
    }
}
