//! Claude / Codex 技能管理：列出、查看、新建、停用、挂载（目录联接）与删除技能。
//!
//! 技能是一个含 `SKILL.md` 的文件夹。客户端从各自的技能目录加载：
//! Claude 为 `~/.claude/skills`，Codex 为 `~/.codex/skills`（其中 `.system` 为内置技能，只读）。
//! 停用的技能移到同级的 `skills-disabled` 目录，客户端不再加载，随时可以移回。
//! 技能库是用户自选的文件夹，库中的技能以目录联接挂载到客户端技能目录，文件只保存一份。
//!
//! 所有写操作都只接受位于上述目录正下方的技能文件夹；删除联接只移除联接本身，
//! 删除真实文件夹则移入回收站，绝不递归删除联接指向的目标。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const SKILL_FILE: &str = "SKILL.md";
const DISABLED_DIR: &str = "skills-disabled";
const CODEX_SYSTEM_DIR: &str = ".system";
/// 统计技能大小时最多遍历的文件数，避免误把大目录当技能时卡住界面。
const MAX_SCAN_FILES: usize = 5000;
/// 预览 SKILL.md 的最大字节数。
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillClient {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillLocationKind {
    Claude,
    Codex,
    Library,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillEntry {
    /// 文件夹名，即客户端加载时使用的技能名。
    pub folder_name: String,
    /// SKILL.md 前言中的 name。
    pub name: Option<String>,
    pub description: Option<String>,
    pub path: String,
    /// 目录联接或符号链接的目标；真实文件夹为空。
    pub link_target: Option<String>,
    pub link_broken: bool,
    pub has_skill_file: bool,
    pub enabled: bool,
    /// Codex 内置技能，只读。
    pub system: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub modified_at: Option<String>,
    /// 技能库中的技能已挂载到哪些客户端。
    pub mounted_in: Vec<SkillClient>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillLocation {
    pub kind: SkillLocationKind,
    pub root: String,
    pub disabled_root: Option<String>,
    pub exists: bool,
    pub skills: Vec<SkillEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillDeleteOutcome {
    LinkRemoved,
    Recycled,
}

/// 技能文件夹在受管目录中的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Placement {
    kind: SkillLocationKind,
    enabled: bool,
    system: bool,
}

pub struct SkillManager {
    claude_home: PathBuf,
    codex_home: PathBuf,
    libraries: Vec<PathBuf>,
}

impl SkillManager {
    pub fn new(claude_home: &Path, codex_home: &Path, libraries: &[String]) -> Self {
        let mut seen = Vec::new();
        let libraries = libraries
            .iter()
            .map(|l| PathBuf::from(crate::util::normalize_windows_path(l.trim())))
            .filter(|l| !l.as_os_str().is_empty())
            .filter(|l| {
                let key = path_key(l);
                let fresh = !seen.contains(&key);
                seen.push(key);
                fresh
            })
            .collect();
        Self {
            claude_home: claude_home.to_path_buf(),
            codex_home: codex_home.to_path_buf(),
            libraries,
        }
    }

    pub fn client_root(&self, client: SkillClient) -> PathBuf {
        match client {
            SkillClient::Claude => self.claude_home.join("skills"),
            SkillClient::Codex => self.codex_home.join("skills"),
        }
    }

    fn disabled_root(&self, client: SkillClient) -> PathBuf {
        match client {
            SkillClient::Claude => self.claude_home.join(DISABLED_DIR),
            SkillClient::Codex => self.codex_home.join(DISABLED_DIR),
        }
    }

    pub fn list(&self) -> Vec<SkillLocation> {
        // 先收集客户端技能目录中的联接，用于标注技能库中的技能挂载到了哪里。
        let mut mounts: HashMap<String, Vec<SkillClient>> = HashMap::new();
        let mut locations = Vec::new();
        for client in [SkillClient::Claude, SkillClient::Codex] {
            let root = self.client_root(client);
            let disabled = self.disabled_root(client);
            let mut skills = scan_dir(&root, true, false);
            skills.extend(scan_dir(&disabled, false, false));
            if client == SkillClient::Codex {
                skills.extend(scan_dir(&root.join(CODEX_SYSTEM_DIR), true, true));
            }
            for skill in &skills {
                if let Some(target) = &skill.link_target {
                    let entry = mounts.entry(path_key(Path::new(target))).or_default();
                    if !entry.contains(&client) {
                        entry.push(client);
                    }
                }
            }
            skills.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
            locations.push(SkillLocation {
                kind: match client {
                    SkillClient::Claude => SkillLocationKind::Claude,
                    SkillClient::Codex => SkillLocationKind::Codex,
                },
                exists: root.is_dir(),
                root: display(&root),
                disabled_root: Some(display(&disabled)),
                skills,
            });
        }
        for library in &self.libraries {
            let mut skills = scan_dir(library, true, false);
            for skill in &mut skills {
                if let Some(clients) = mounts.get(&path_key(Path::new(&skill.path))) {
                    skill.mounted_in = clients.clone();
                }
            }
            skills.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
            locations.push(SkillLocation {
                kind: SkillLocationKind::Library,
                exists: library.is_dir(),
                root: display(library),
                disabled_root: None,
                skills,
            });
        }
        locations
    }

    /// 读取技能的 SKILL.md 全文（联接按目标读取）。
    pub fn read_skill_file(&self, path: &str) -> Result<String, String> {
        let (dir, _) = self.locate(path)?;
        let file = dir.join(SKILL_FILE);
        let meta = fs::metadata(&file).map_err(|_| format!("该技能没有 {}：{}", SKILL_FILE, dir.display()))?;
        if meta.len() > MAX_PREVIEW_BYTES {
            return Err(format!("{} 超过 {} KB，请用编辑器打开查看", SKILL_FILE, MAX_PREVIEW_BYTES / 1024));
        }
        let bytes = fs::read(&file).map_err(|e| format!("读取 {} 失败：{}", file.display(), e))?;
        Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
    }

    /// 新建技能文件夹和 SKILL.md。`target` 为 claude、codex 或技能库路径。
    pub fn create(&self, target: &str, folder_name: &str, description: &str) -> Result<PathBuf, String> {
        validate_skill_name(folder_name)?;
        let description = description.trim();
        if description.is_empty() {
            return Err("请填写技能简介，客户端靠它判断何时使用该技能".into());
        }
        let root = match target {
            "claude" => self.client_root(SkillClient::Claude),
            "codex" => self.client_root(SkillClient::Codex),
            other => {
                let wanted = path_key(Path::new(&crate::util::normalize_windows_path(other)));
                self.libraries
                    .iter()
                    .find(|l| path_key(l) == wanted)
                    .cloned()
                    .ok_or_else(|| format!("未登记的技能库：{}", other))?
            }
        };
        if self.libraries.iter().any(|l| path_key(l) == path_key(&root)) && !root.is_dir() {
            return Err(format!("技能库文件夹不存在：{}", root.display()));
        }
        fs::create_dir_all(&root).map_err(|e| format!("无法创建技能目录 {}：{}", root.display(), e))?;
        let dir = root.join(folder_name);
        if fs::symlink_metadata(&dir).is_ok() {
            return Err(format!("已存在同名技能：{}", dir.display()));
        }
        fs::create_dir(&dir).map_err(|e| format!("无法创建技能文件夹 {}：{}", dir.display(), e))?;
        let content = format!(
            "---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n\n在这里写明这个技能要做什么、按什么步骤做、有哪些注意事项。\n",
            name = folder_name,
            desc = yaml_scalar(description),
        );
        fs::write(dir.join(SKILL_FILE), content).map_err(|e| format!("写入 {} 失败：{}", SKILL_FILE, e))?;
        Ok(dir)
    }

    /// 在客户端技能目录与 skills-disabled 之间移动技能，返回新路径。
    pub fn set_enabled(&self, path: &str, enabled: bool) -> Result<PathBuf, String> {
        let source = PathBuf::from(crate::util::normalize_windows_path(path));
        let placement = self.placement(&source)?;
        let client = match placement.kind {
            SkillLocationKind::Claude => SkillClient::Claude,
            SkillLocationKind::Codex => SkillClient::Codex,
            SkillLocationKind::Library => return Err("技能库中的技能不直接被加载，请用挂载或取消挂载控制".into()),
        };
        if placement.system {
            return Err("Codex 内置技能为只读，不能停用".into());
        }
        if placement.enabled == enabled {
            return Ok(source);
        }
        let target_root = if enabled { self.client_root(client) } else { self.disabled_root(client) };
        fs::create_dir_all(&target_root)
            .map_err(|e| format!("无法创建目录 {}：{}", target_root.display(), e))?;
        let target = target_root.join(file_name(&source)?);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(format!("目标位置已有同名技能：{}", target.display()));
        }
        // 同一卷内重命名：联接只移动联接本身，不触及目标。
        fs::rename(&source, &target).map_err(|e| format!("移动技能失败：{}", e))?;
        Ok(target)
    }

    /// 把技能库中的技能以目录联接挂载到客户端技能目录。
    pub fn mount(&self, path: &str, client: SkillClient) -> Result<PathBuf, String> {
        let source = PathBuf::from(crate::util::normalize_windows_path(path));
        let placement = self.placement(&source)?;
        if placement.kind != SkillLocationKind::Library {
            return Err("只能挂载技能库中的技能".into());
        }
        if is_reparse_point(&source) {
            return Err("该技能本身是一个联接，请挂载它指向的原始文件夹".into());
        }
        if !source.join(SKILL_FILE).is_file() {
            return Err(format!("该文件夹没有 {}，不是有效技能", SKILL_FILE));
        }
        let name = file_name(&source)?;
        let root = self.client_root(client);
        for existing in [root.join(&name), self.disabled_root(client).join(&name)] {
            if fs::symlink_metadata(&existing).is_ok() {
                return Err(format!("客户端中已有同名技能：{}", existing.display()));
            }
        }
        fs::create_dir_all(&root).map_err(|e| format!("无法创建技能目录 {}：{}", root.display(), e))?;
        let link = root.join(&name);
        create_junction(&link, &source)?;
        Ok(link)
    }

    /// 删除技能：联接只移除联接本身；真实文件夹移入回收站。
    pub fn delete(&self, path: &str) -> Result<SkillDeleteOutcome, String> {
        let target = PathBuf::from(crate::util::normalize_windows_path(path));
        let placement = self.placement(&target)?;
        if placement.system {
            return Err("Codex 内置技能为只读，不能删除".into());
        }
        if is_reparse_point(&target) {
            remove_link(&target)?;
            return Ok(SkillDeleteOutcome::LinkRemoved);
        }
        move_to_recycle_bin(&target)?;
        Ok(SkillDeleteOutcome::Recycled)
    }

    /// 校验并解析一个技能路径，返回可读取内容的实际目录（联接取目标）。
    fn locate(&self, path: &str) -> Result<(PathBuf, Placement), String> {
        let dir = PathBuf::from(crate::util::normalize_windows_path(path));
        let placement = self.placement(&dir)?;
        let real = if is_reparse_point(&dir) {
            fs::read_link(&dir)
                .map(|t| PathBuf::from(strip_verbatim(&t)))
                .map_err(|e| format!("无法读取联接目标：{}", e))?
        } else {
            dir
        };
        Ok((real, placement))
    }

    /// 技能文件夹必须直接位于受管目录之下，防止误操作其他位置。
    fn placement(&self, dir: &Path) -> Result<Placement, String> {
        let parent = dir
            .parent()
            .ok_or_else(|| format!("无效的技能路径：{}", dir.display()))?;
        if fs::symlink_metadata(dir).is_err() {
            return Err(format!("技能不存在：{}", dir.display()));
        }
        let parent_key = path_key(parent);
        for (client, kind) in [
            (SkillClient::Claude, SkillLocationKind::Claude),
            (SkillClient::Codex, SkillLocationKind::Codex),
        ] {
            let root = self.client_root(client);
            if parent_key == path_key(&root) {
                return Ok(Placement { kind, enabled: true, system: false });
            }
            if parent_key == path_key(&self.disabled_root(client)) {
                return Ok(Placement { kind, enabled: false, system: false });
            }
            if client == SkillClient::Codex && parent_key == path_key(&root.join(CODEX_SYSTEM_DIR)) {
                return Ok(Placement { kind, enabled: true, system: true });
            }
        }
        if self.libraries.iter().any(|l| path_key(l) == parent_key) {
            return Ok(Placement { kind: SkillLocationKind::Library, enabled: true, system: false });
        }
        Err(format!("不在受管的技能目录中：{}", dir.display()))
    }
}

/// 列出目录下的技能文件夹（只看一层）。
fn scan_dir(root: &Path, enabled: bool, system: bool) -> Vec<SkillEntry> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // .system 由 Codex 单独列出；其他隐藏项（如 .codex-system-skills.marker）不是技能。
        if name.starts_with('.') {
            continue;
        }
        let link = is_reparse_point(&path);
        let link_target = if link { fs::read_link(&path).ok().map(|t| strip_verbatim(&t)) } else { None };
        let real = link_target.as_ref().map(PathBuf::from).unwrap_or_else(|| path.clone());
        if !link && !path.is_dir() {
            continue;
        }
        let link_broken = link && !real.is_dir();
        let skill_file = real.join(SKILL_FILE);
        let has_skill_file = skill_file.is_file();
        let (front_name, description) = if has_skill_file {
            fs::read(&skill_file)
                .map(|b| parse_frontmatter(&String::from_utf8_lossy(&b)))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };
        let (file_count, total_bytes) = if link_broken { (0, 0) } else { dir_size(&real) };
        let modified_at = fs::metadata(if has_skill_file { &skill_file } else { &real })
            .and_then(|m| m.modified())
            .ok()
            .map(|t| DateTime::<Utc>::from(t).to_rfc3339());
        skills.push(SkillEntry {
            folder_name: name,
            name: front_name,
            description,
            path: display(&path),
            link_target: link_target.clone(),
            link_broken,
            has_skill_file,
            enabled,
            system,
            file_count,
            total_bytes,
            modified_at,
            mounted_in: Vec::new(),
        });
    }
    skills
}

fn sort_key(s: &SkillEntry) -> (bool, bool, String) {
    (s.system, !s.enabled, s.folder_name.to_lowercase())
}

/// 文件数与总字节数；不跟随技能内部的嵌套联接。
fn dir_size(root: &Path) -> (usize, u64) {
    let mut stack = vec![root.to_path_buf()];
    let (mut count, mut bytes) = (0usize, 0u64);
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            if is_reparse_point(&path) {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else {
                count += 1;
                bytes += meta.len();
                if count >= MAX_SCAN_FILES {
                    return (count, bytes);
                }
            }
        }
    }
    (count, bytes)
}

/// 解析 SKILL.md 开头 `---` 之间的 name 与 description（支持引号和 `>`、`|` 多行写法）。
pub fn parse_frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let body: Vec<&str> = lines.take_while(|l| l.trim() != "---").collect();
    let mut values: HashMap<&str, String> = HashMap::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        i += 1;
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else { continue };
        let key = key.trim();
        if key != "name" && key != "description" {
            continue;
        }
        let rest = rest.trim();
        let value = if rest.starts_with('>') || rest.starts_with('|') {
            let mut parts = Vec::new();
            while i < body.len() && (body[i].trim().is_empty() || body[i].starts_with(char::is_whitespace)) {
                parts.push(body[i].trim());
                i += 1;
            }
            let sep = if rest.starts_with('|') { "\n" } else { " " };
            parts.join(sep).trim().to_string()
        } else {
            unquote(rest)
        };
        if !value.is_empty() {
            values.insert(if key == "name" { "name" } else { "description" }, value);
        }
    }
    (values.remove("name"), values.remove("description"))
}

fn unquote(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        return raw[1..raw.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\");
    }
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return raw[1..raw.len() - 1].replace("''", "'");
    }
    raw.to_string()
}

/// 单行写入 YAML；含特殊字符时用双引号。
fn yaml_scalar(value: &str) -> String {
    let single_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let needs_quotes = single_line.contains(": ")
        || single_line.contains(" #")
        || single_line.starts_with(|c: char| "\"'#&*!|>%@`[]{},-?".contains(c));
    if needs_quotes {
        format!("\"{}\"", single_line.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        single_line
    }
}

/// 技能文件夹名：小写字母、数字和连字符，与客户端的技能命名规则一致。
pub fn validate_skill_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err("技能名只能包含小写字母、数字和连字符（如 my-skill），不超过 64 个字符".into())
    }
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("无效的技能路径：{}", path.display()))
}

fn display(path: &Path) -> String {
    strip_verbatim(path)
}

/// `read_link` 返回 `\\?\E:\...` 形式，界面和比较时去掉前缀。
fn strip_verbatim(path: &Path) -> String {
    let text = path.to_string_lossy().to_string();
    text.strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{}", rest))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(text)
}

/// 路径比较键：统一分隔符、去掉末尾分隔符并忽略大小写（Windows 路径不区分大小写）。
fn path_key(path: &Path) -> String {
    strip_verbatim(path)
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(windows)]
pub fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    fs::symlink_metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_reparse_point(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

/// 移除联接或目录符号链接本身；目标文件夹不受影响。
fn remove_link(link: &Path) -> Result<(), String> {
    if !is_reparse_point(link) {
        return Err(format!("不是联接，拒绝按联接删除：{}", link.display()));
    }
    fs::remove_dir(link)
        .or_else(|_| fs::remove_file(link))
        .map_err(|e| format!("移除联接失败：{}", e))
}

/// 通过 PowerShell 执行；路径经环境变量传入，不会成为脚本的一部分。
#[cfg(windows)]
fn run_powershell(script: &str, env: &[(&str, &Path)]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $ErrorActionPreference = 'Stop'; {script}"),
        ])
        .creation_flags(CREATE_NO_WINDOW);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().map_err(|e| format!("无法启动 PowerShell：{}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn create_junction(link: &Path, target: &Path) -> Result<(), String> {
    run_powershell(
        "New-Item -ItemType Junction -Path $env:CV_LINK -Target $env:CV_TARGET | Out-Null",
        &[("CV_LINK", link), ("CV_TARGET", target)],
    )
    .map_err(|e| format!("创建目录联接失败：{}", e))?;
    if is_reparse_point(link) {
        Ok(())
    } else {
        Err(format!("创建目录联接失败：{}", link.display()))
    }
}

#[cfg(not(windows))]
fn create_junction(link: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| format!("创建链接失败：{}", e))
}

#[cfg(windows)]
fn move_to_recycle_bin(dir: &Path) -> Result<(), String> {
    run_powershell(
        "Add-Type -AssemblyName Microsoft.VisualBasic; \
         [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($env:CV_TARGET, 'OnlyErrorDialogs', 'SendToRecycleBin')",
        &[("CV_TARGET", dir)],
    )
    .map_err(|e| format!("移入回收站失败：{}", e))?;
    if fs::symlink_metadata(dir).is_ok() {
        return Err(format!("移入回收站失败：{}", dir.display()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn move_to_recycle_bin(dir: &Path) -> Result<(), String> {
    Err(format!("当前系统不支持移入回收站：{}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("cv-skills-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            // 先移除测试中创建的联接，避免递归删除时进入目标。
            for root in ["claude/skills", "claude/skills-disabled", "codex/skills"] {
                if let Ok(entries) = fs::read_dir(self.0.join(root)) {
                    for e in entries.flatten() {
                        if is_reparse_point(&e.path()) {
                            let _ = remove_link(&e.path());
                        }
                    }
                }
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn skill(root: &Path, name: &str, front: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SKILL_FILE), front).unwrap();
        dir
    }

    fn manager(t: &TempDir, libraries: &[&Path]) -> SkillManager {
        let libs: Vec<String> = libraries.iter().map(|p| p.to_string_lossy().to_string()).collect();
        SkillManager::new(&t.0.join("claude"), &t.0.join("codex"), &libs)
    }

    #[test]
    fn frontmatter_reads_plain_quoted_and_folded_values() {
        let (n, d) = parse_frontmatter("\u{feff}---\nname: demo\ndescription: \"Use: when \\\"needed\\\"\"\n---\nbody");
        assert_eq!(n.as_deref(), Some("demo"));
        assert_eq!(d.as_deref(), Some("Use: when \"needed\""));
        let (_, d) = parse_frontmatter("---\nname: x\ndescription: >\n  first line\n  second line\nmetadata:\n  a: b\n---\n");
        assert_eq!(d.as_deref(), Some("first line second line"));
        assert_eq!(parse_frontmatter("# no frontmatter"), (None, None));
    }

    #[test]
    fn names_follow_client_rules() {
        assert!(validate_skill_name("word-editing-preferences").is_ok());
        for bad in ["", "Word", "a b", "-a", "a-", "中文", &"x".repeat(65)] {
            assert!(validate_skill_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn lists_clients_disabled_system_and_libraries() {
        let t = TempDir::new();
        skill(&t.0.join("claude/skills"), "alpha", "---\nname: alpha\ndescription: A\n---\n");
        skill(&t.0.join("claude/skills-disabled"), "beta", "---\nname: beta\ndescription: B\n---\n");
        skill(&t.0.join("codex/skills/.system"), "imagegen", "---\nname: imagegen\ndescription: I\n---\n");
        fs::write(t.0.join("codex/skills/.codex-system-skills.marker"), "").unwrap();
        fs::create_dir_all(t.0.join("lib/empty-folder")).unwrap();
        let lib = t.0.join("lib");
        let locations = manager(&t, &[&lib]).list();
        assert_eq!(locations.len(), 3);
        let claude = &locations[0];
        assert_eq!(claude.skills.iter().map(|s| (s.folder_name.as_str(), s.enabled)).collect::<Vec<_>>(),
                   vec![("alpha", true), ("beta", false)]);
        let codex = &locations[1];
        assert_eq!(codex.skills.len(), 1);
        assert!(codex.skills[0].system);
        let library = &locations[2];
        assert_eq!(library.skills[0].folder_name, "empty-folder");
        assert!(!library.skills[0].has_skill_file);
    }

    #[test]
    fn create_disable_enable_round_trip() {
        let t = TempDir::new();
        let m = manager(&t, &[]);
        let created = m.create("claude", "my-skill", "Use when: testing").unwrap();
        let text = fs::read_to_string(created.join(SKILL_FILE)).unwrap();
        assert_eq!(parse_frontmatter(&text), (Some("my-skill".into()), Some("Use when: testing".into())));
        assert!(m.create("claude", "my-skill", "again").is_err());
        let disabled = m.set_enabled(&created.to_string_lossy(), false).unwrap();
        assert!(disabled.starts_with(t.0.join("claude/skills-disabled")));
        let enabled = m.set_enabled(&disabled.to_string_lossy(), true).unwrap();
        assert_eq!(enabled, created);
        assert!(m.read_skill_file(&enabled.to_string_lossy()).unwrap().contains("# my-skill"));
    }

    #[test]
    fn paths_outside_managed_roots_are_rejected() {
        let t = TempDir::new();
        let outside = skill(&t.0.join("elsewhere"), "victim", "---\nname: victim\n---\n");
        let m = manager(&t, &[]);
        assert!(m.delete(&outside.to_string_lossy()).is_err());
        assert!(m.set_enabled(&outside.to_string_lossy(), false).is_err());
        assert!(m.read_skill_file(&outside.to_string_lossy()).is_err());
        assert!(m.create(&t.0.join("elsewhere").to_string_lossy(), "x", "d").is_err());
        assert!(outside.join(SKILL_FILE).is_file());
    }

    #[test]
    fn mount_marks_library_and_removing_link_keeps_source() {
        let t = TempDir::new();
        let lib = t.0.join("lib");
        let source = skill(&lib, "shared", "---\nname: shared\ndescription: S\n---\n");
        fs::write(source.join("notes.txt"), "keep me").unwrap();
        let m = manager(&t, &[&lib]);
        let link = m.mount(&source.to_string_lossy(), SkillClient::Claude).unwrap();
        assert!(is_reparse_point(&link));
        assert!(m.mount(&source.to_string_lossy(), SkillClient::Claude).is_err());

        let locations = m.list();
        let mounted = &locations[2].skills[0];
        assert_eq!(mounted.mounted_in, vec![SkillClient::Claude]);
        let entry = &locations[0].skills[0];
        assert_eq!(entry.link_target.as_deref().map(|p| path_key(Path::new(p))), Some(path_key(&source)));
        assert_eq!(entry.name.as_deref(), Some("shared"));
        assert!(m.read_skill_file(&link.to_string_lossy()).unwrap().contains("description: S"));

        // 停用后联接移到 skills-disabled，仍指向原文件夹。
        let disabled = m.set_enabled(&link.to_string_lossy(), false).unwrap();
        assert!(is_reparse_point(&disabled));
        assert_eq!(m.delete(&disabled.to_string_lossy()).unwrap(), SkillDeleteOutcome::LinkRemoved);
        assert!(fs::symlink_metadata(&disabled).is_err());
        assert_eq!(fs::read_to_string(source.join("notes.txt")).unwrap(), "keep me");
        assert!(m.list()[2].skills[0].mounted_in.is_empty());
    }
}
