//! Claude Code / Codex 的 MCP 服务器管理：列出、新建、编辑、启用/停用、删除和跨客户端复制。
//!
//! 配置文件就是唯一的数据来源：
//! - Claude Code：`~/.claude.json` 顶层的 `mcpServers`（用户级）。该文件没有停用开关，
//!   停用的条目移到本程序数据目录的 `mcp-disabled/claude.json`，启用时写回。
//! - Codex：`~/.codex/config.toml` 的 `[mcp_servers.<名字>]`，停用用 Codex 原生的 `enabled = false`。
//!
//! 界面统一用 Claude 的 JSON 写法（type、command、args、env、cwd、url、headers）表示一个服务器，
//! 写入 Codex 时转换为 TOML：不写 `type`，headers 写作 `http_headers`，且只写所选传输方式的字段，
//! 否则 Codex 会拒绝加载整份配置。
//!
//! 每次写入前把原文件备份到 `backups/mcp`；写入前再核对文件是否被其他程序改过（Claude Code
//! 运行时会频繁改写 `~/.claude.json`），改过就重新读取后再改；写入用临时文件替换，避免写坏。
//! `~/.claude.json` 只改 `mcpServers`，其他字段和键顺序保持不变；`config.toml` 只改对应的表，保留注释。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, InlineTable, Item, Table};

use super::cc_switch::CcSwitchState;

/// Codex 桌面版自动生成并维护的条目，只读。
const CODEX_APP_MANAGED: &[&str] = &["node_repl"];
const MAX_BACKUPS: usize = 30;
const WRITE_ATTEMPTS: usize = 5;
const STASH_FILE: &str = "claude.json";

/// Claude Code 认识的字段；复制到 Claude 时只保留这些。
const CLAUDE_STDIO_FIELDS: &[&str] = &["type", "command", "args", "env"];
const CLAUDE_REMOTE_FIELDS: &[&str] = &["type", "url", "headers", "headersHelper", "oauth"];
/// Codex 认识的字段（JSON 写法，headers 对应 http_headers）；复制到 Codex 时只保留这些。
const CODEX_COMMON_FIELDS: &[&str] = &[
    "type",
    "enabled",
    "required",
    "startup_timeout_sec",
    "startup_timeout_ms",
    "tool_timeout_sec",
    "enabled_tools",
    "disabled_tools",
    "scopes",
];
const CODEX_STDIO_FIELDS: &[&str] = &["command", "args", "env", "env_vars", "cwd"];
const CODEX_REMOTE_FIELDS: &[&str] = &[
    "url",
    "headers",
    "env_http_headers",
    "bearer_token_env_var",
    "oauth_resource",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpClient {
    Claude,
    Codex,
}

impl McpClient {
    fn label(self) -> &'static str {
        match self {
            McpClient::Claude => "Claude",
            McpClient::Codex => "Codex",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerEntry {
    pub client: McpClient,
    pub name: String,
    /// stdio、http 或 sse。
    pub transport: String,
    /// Claude 的 JSON 写法，含 type 和其他所有字段。
    pub spec: Value,
    pub enabled: bool,
    /// Codex 桌面版生成的条目，只读。
    pub read_only: bool,
    /// cc-switch 登记了同名服务器时，它对该客户端是否启用；未登记为空。
    pub cc_switch: Option<bool>,
    /// 配置文件中的原始片段。
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpLocation {
    pub client: McpClient,
    pub config_path: String,
    pub exists: bool,
    /// 配置文件无法解析时的错误，此时界面不提供写操作。
    pub error: Option<String>,
    pub servers: Vec<McpServerEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCopyOutcome {
    /// 目标客户端不支持、复制时舍弃的字段。
    pub dropped_fields: Vec<String>,
}

pub struct McpManager {
    claude_json: PathBuf,
    codex_home: PathBuf,
    stash_dir: PathBuf,
    backups: PathBuf,
}

impl McpManager {
    /// `data_dir` 存放 Claude 停用条目，`backups` 下的 `mcp` 子目录存放配置备份。
    pub fn new(claude_json: &Path, codex_home: &Path, data_dir: &Path, backups: &Path) -> Self {
        Self {
            claude_json: claude_json.to_path_buf(),
            codex_home: codex_home.to_path_buf(),
            stash_dir: data_dir.join("mcp-disabled"),
            backups: backups.join("mcp"),
        }
    }

    fn codex_config(&self) -> PathBuf {
        self.codex_home.join("config.toml")
    }

    fn stash_path(&self) -> PathBuf {
        self.stash_dir.join(STASH_FILE)
    }

    pub fn list(&self, cc_switch: &CcSwitchState) -> Vec<McpLocation> {
        let mark = |client: McpClient, name: &str| {
            cc_switch.mcp_servers.get(name).map(|apps| match client {
                McpClient::Claude => apps.claude,
                McpClient::Codex => apps.codex,
            })
        };
        let mut locations = vec![self.list_claude(), self.list_codex()];
        for location in &mut locations {
            for server in &mut location.servers {
                server.cc_switch = mark(server.client, &server.name);
            }
            location.servers.sort_by_key(|s| (s.name.to_lowercase(), !s.enabled));
        }
        locations
    }

    fn list_claude(&self) -> McpLocation {
        let exists = self.claude_json.is_file();
        let mut servers = Vec::new();
        let mut error = None;
        if exists {
            match read_json_object(&self.claude_json) {
                Ok(root) => match root.get("mcpServers") {
                    None => {}
                    Some(Value::Object(map)) => {
                        servers.extend(map.iter().map(|(name, spec)| claude_entry(name, spec, true)));
                    }
                    Some(_) => error = Some("mcpServers 不是对象，请手动检查 .claude.json".into()),
                },
                Err(e) => error = Some(e),
            }
        }
        match self.read_stash() {
            Ok(stash) => servers.extend(stash.iter().map(|(name, spec)| claude_entry(name, spec, false))),
            Err(e) => error = error.or(Some(e)),
        }
        McpLocation {
            client: McpClient::Claude,
            config_path: self.claude_json.to_string_lossy().to_string(),
            exists,
            error,
            servers,
        }
    }

    fn list_codex(&self) -> McpLocation {
        let path = self.codex_config();
        let exists = path.is_file();
        let mut servers = Vec::new();
        let mut error = None;
        if exists {
            match read_toml(&path) {
                Ok(doc) => match doc.get("mcp_servers") {
                    None => {}
                    Some(item) => match item.as_table_like() {
                        Some(table) => {
                            for (name, item) in table.iter() {
                                match codex_entry(name, item) {
                                    Some(entry) => servers.push(entry),
                                    None => error = Some(format!("[mcp_servers.{}] 不是表，已跳过", name)),
                                }
                            }
                        }
                        None => error = Some("mcp_servers 不是表，请手动检查 config.toml".into()),
                    },
                },
                Err(e) => error = Some(e),
            }
        }
        McpLocation {
            client: McpClient::Codex,
            config_path: path.to_string_lossy().to_string(),
            exists,
            error,
            servers,
        }
    }

    /// 新建（`original` 为空）或编辑一个服务器；编辑时可改名，启用状态保持不变。
    pub fn upsert(&self, client: McpClient, original: Option<&str>, name: &str, spec: &Value) -> Result<(), String> {
        if original != Some(name) {
            validate_name(name)?;
        }
        validate_spec(spec)?;
        match client {
            McpClient::Claude => self.upsert_claude(original, name, spec),
            McpClient::Codex => self.upsert_codex(original, name, spec),
        }
    }

    fn upsert_claude(&self, original: Option<&str>, name: &str, spec: &Value) -> Result<(), String> {
        let mut stash = self.read_stash()?;
        let in_stash = original.is_some_and(|o| stash.contains_key(o));
        let renamed = original != Some(name);
        if renamed && stash.contains_key(name) {
            return Err(format!("已有同名的停用条目：{}", name));
        }
        if in_stash {
            // 编辑停用的条目只改暂存文件。
            if renamed && self.claude_live_names()?.iter().any(|n| n == name) {
                return Err(format!("Claude 中已有同名服务器：{}", name));
            }
            replace_key(&mut stash, original.unwrap(), name, spec.clone());
            return self.write_stash(&stash);
        }
        self.edit_claude(|servers| {
            if let Some(original) = original {
                if !servers.contains_key(original) {
                    return Err(format!("Claude 中没有这个服务器：{}", original));
                }
            }
            if renamed && servers.contains_key(name) {
                return Err(format!("Claude 中已有同名服务器：{}", name));
            }
            replace_key(servers, original.unwrap_or(name), name, spec.clone());
            Ok(())
        })
    }

    fn upsert_codex(&self, original: Option<&str>, name: &str, spec: &Value) -> Result<(), String> {
        for n in original.iter().chain(std::iter::once(&name)) {
            ensure_writable(McpClient::Codex, n)?;
        }
        let (table, _) = spec_to_codex_table(spec, false)?;
        self.edit_codex(|doc| {
            let servers = codex_servers_mut(doc)?;
            let mut table = table.clone();
            if let Some(original) = original {
                let old = servers
                    .get(original)
                    .ok_or_else(|| format!("Codex 中没有这个服务器：{}", original))?;
                if let Some(old) = old.as_table_like() {
                    if old.get("enabled").and_then(Item::as_bool) == Some(false) {
                        table.insert("enabled", toml_edit::value(false));
                    }
                }
                if let Some(old) = old.as_table() {
                    table.set_position(old.position());
                }
            }
            if original != Some(name) && servers.contains_key(name) {
                return Err(format!("Codex 中已有同名服务器：{}", name));
            }
            if let Some(original) = original {
                servers.remove(original);
            }
            servers.insert(name, Item::Table(table));
            Ok(())
        })
    }

    pub fn set_enabled(&self, client: McpClient, name: &str, enabled: bool) -> Result<(), String> {
        ensure_writable(client, name)?;
        match client {
            McpClient::Claude if enabled => {
                let mut stash = self.read_stash()?;
                let spec = stash
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("没有找到停用的 Claude 服务器：{}", name))?;
                self.edit_claude(|servers| {
                    if servers.contains_key(name) {
                        return Err(format!("Claude 中已有同名的启用服务器：{}", name));
                    }
                    servers.insert(name.to_string(), spec.clone());
                    Ok(())
                })?;
                stash.shift_remove(name);
                self.write_stash(&stash)
            }
            McpClient::Claude => {
                let stash_before = self.read_stash()?;
                // 先把条目写进暂存文件，再从 .claude.json 移除，任何一步失败都不会丢失配置。
                let result = self.edit_claude(|servers| {
                    let spec = servers
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Claude 中没有这个启用的服务器：{}", name))?;
                    let mut stash = stash_before.clone();
                    stash.insert(name.to_string(), spec);
                    self.write_stash(&stash)?;
                    servers.shift_remove(name);
                    Ok(())
                });
                if result.is_err() {
                    let _ = self.write_stash(&stash_before);
                }
                result
            }
            McpClient::Codex => self.edit_codex(|doc| {
                let server = codex_servers_mut(doc)?
                    .get_mut(name)
                    .and_then(Item::as_table_like_mut)
                    .ok_or_else(|| format!("Codex 中没有这个服务器：{}", name))?;
                if enabled {
                    server.remove("enabled");
                } else {
                    server.insert("enabled", toml_edit::value(false));
                }
                Ok(())
            }),
        }
    }

    pub fn delete(&self, client: McpClient, name: &str) -> Result<(), String> {
        ensure_writable(client, name)?;
        match client {
            McpClient::Claude => {
                if self.claude_live_names()?.iter().any(|n| n == name) {
                    return self.edit_claude(|servers| {
                        servers.shift_remove(name);
                        Ok(())
                    });
                }
                let mut stash = self.read_stash()?;
                if stash.shift_remove(name).is_none() {
                    return Err(format!("Claude 中没有这个服务器：{}", name));
                }
                self.write_stash(&stash)
            }
            McpClient::Codex => self.edit_codex(|doc| {
                codex_servers_mut(doc)?
                    .remove(name)
                    .map(|_| ())
                    .ok_or_else(|| format!("Codex 中没有这个服务器：{}", name))
            }),
        }
    }

    /// 把一个服务器复制到另一个客户端（启用状态）。目标已有同名条目时，只有 `overwrite` 才覆盖启用的条目；
    /// 目标中有同名的停用条目时总是拒绝，避免出现两份。
    pub fn copy(&self, from: McpClient, name: &str, to: McpClient, overwrite: bool) -> Result<McpCopyOutcome, String> {
        if from == to {
            return Err("源和目标是同一个客户端".into());
        }
        ensure_writable(from, name)?;
        ensure_writable(to, name)?;
        let source = self
            .list(&CcSwitchState::default())
            .into_iter()
            .find(|l| l.client == from)
            .and_then(|l| l.servers.into_iter().find(|s| s.name == name))
            .ok_or_else(|| format!("{} 中没有这个服务器：{}", from.label(), name))?;
        validate_spec(&source.spec)?;
        let target_entry = self
            .list(&CcSwitchState::default())
            .into_iter()
            .find(|l| l.client == to)
            .and_then(|l| l.servers.into_iter().find(|s| s.name == name));
        if let Some(existing) = target_entry {
            if !existing.enabled {
                return Err(format!("{} 中有同名的停用条目，请先启用或删除它", to.label()));
            }
            if !overwrite {
                return Err(format!("{} 中已有同名服务器：{}", to.label(), name));
            }
        }
        match to {
            McpClient::Claude => {
                let (spec, dropped_fields) = spec_for_claude(&source.spec);
                self.edit_claude(|servers| {
                    replace_key(servers, name, name, spec.clone());
                    Ok(())
                })?;
                Ok(McpCopyOutcome { dropped_fields })
            }
            McpClient::Codex => {
                let (table, dropped_fields) = spec_to_codex_table(&source.spec, true)?;
                self.edit_codex(|doc| {
                    let servers = codex_servers_mut(doc)?;
                    let mut table = table.clone();
                    if let Some(old) = servers.get(name).and_then(Item::as_table) {
                        table.set_position(old.position());
                    }
                    servers.insert(name, Item::Table(table));
                    Ok(())
                })?;
                Ok(McpCopyOutcome { dropped_fields })
            }
        }
    }

    fn claude_live_names(&self) -> Result<Vec<String>, String> {
        if !self.claude_json.is_file() {
            return Ok(Vec::new());
        }
        let root = read_json_object(&self.claude_json)?;
        Ok(root
            .get("mcpServers")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default())
    }

    fn read_stash(&self) -> Result<Map<String, Value>, String> {
        let path = self.stash_path();
        if !path.is_file() {
            return Ok(Map::new());
        }
        let root = read_json_object(&path)?;
        Ok(root.get("servers").and_then(Value::as_object).cloned().unwrap_or_default())
    }

    fn write_stash(&self, servers: &Map<String, Value>) -> Result<(), String> {
        fs::create_dir_all(&self.stash_dir).map_err(|e| format!("无法创建 {}：{}", self.stash_dir.display(), e))?;
        let text = serde_json::to_string_pretty(&json!({ "version": 1, "servers": servers }))
            .map_err(|e| e.to_string())?;
        atomic_write(&self.stash_path(), text.as_bytes())
    }

    /// 读取 `.claude.json`，修改 `mcpServers` 后写回。文件在修改期间被其他程序改过就重来。
    fn edit_claude<F>(&self, mut edit: F) -> Result<(), String>
    where
        F: FnMut(&mut Map<String, Value>) -> Result<(), String>,
    {
        let path = &self.claude_json;
        if !path.is_file() {
            return Err(format!("没有找到 {}，请先运行一次 Claude Code", path.display()));
        }
        for _ in 0..WRITE_ATTEMPTS {
            let (bytes, stamp) = read_with_stamp(path)?;
            let mut root = parse_json_object(&bytes, path)?;
            let servers = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
            let servers = servers
                .as_object_mut()
                .ok_or_else(|| "mcpServers 不是对象，请手动检查 .claude.json".to_string())?;
            edit(servers)?;
            let mut text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
            if bytes.ends_with(b"\n") {
                text.push('\n');
            }
            if file_stamp(path) != Some(stamp) {
                continue;
            }
            self.backup("claude.json", &bytes)?;
            if atomic_write(path, text.as_bytes()).is_ok() {
                return Ok(());
            }
        }
        Err(format!("{} 正被其他程序频繁改写，请稍后再试", path.display()))
    }

    fn edit_codex<F>(&self, mut edit: F) -> Result<(), String>
    where
        F: FnMut(&mut DocumentMut) -> Result<(), String>,
    {
        if !self.codex_home.is_dir() {
            return Err(format!("没有找到 Codex 配置目录 {}，请先运行一次 Codex", self.codex_home.display()));
        }
        let path = self.codex_config();
        for _ in 0..WRITE_ATTEMPTS {
            let (bytes, stamp) = if path.is_file() {
                let (b, s) = read_with_stamp(&path)?;
                (b, Some(s))
            } else {
                (Vec::new(), None)
            };
            let mut doc = parse_toml(&bytes, &path)?;
            edit(&mut doc)?;
            let text = doc.to_string();
            if file_stamp(&path) != stamp {
                continue;
            }
            if stamp.is_some() {
                self.backup("codex-config.toml", &bytes)?;
            }
            if atomic_write(&path, text.as_bytes()).is_ok() {
                return Ok(());
            }
        }
        Err(format!("{} 正被其他程序频繁改写，请稍后再试", path.display()))
    }

    /// 备份原文件，只保留最近 [`MAX_BACKUPS`] 份。
    fn backup(&self, prefix: &str, bytes: &[u8]) -> Result<(), String> {
        fs::create_dir_all(&self.backups).map_err(|e| format!("无法创建备份目录 {}：{}", self.backups.display(), e))?;
        let file = self
            .backups
            .join(format!("{}-{}.bak", prefix, Local::now().format("%Y%m%d-%H%M%S-%3f")));
        fs::write(&file, bytes).map_err(|e| format!("备份 {} 失败：{}", file.display(), e))?;
        let mut existing: Vec<PathBuf> = fs::read_dir(&self.backups)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{}-", prefix))))
                    .collect()
            })
            .unwrap_or_default();
        existing.sort();
        let excess = existing.len().saturating_sub(MAX_BACKUPS);
        for old in existing.into_iter().take(excess) {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }
}

fn ensure_writable(client: McpClient, name: &str) -> Result<(), String> {
    if client == McpClient::Codex && CODEX_APP_MANAGED.contains(&name) {
        return Err(format!("{} 由 Codex 桌面版自动生成和维护，这里只读", name));
    }
    Ok(())
}

/// 服务器名：字母、数字、连字符和下划线，两个客户端都能直接使用。
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err("服务器名只能包含字母、数字、连字符和下划线（如 my-server），不超过 64 个字符".into())
    }
}

/// stdio、http 或 sse：显式的 type 优先，否则有 url 为 http。
pub fn spec_transport(spec: &Value) -> String {
    match spec.get("type").and_then(Value::as_str) {
        Some(t) => t.to_string(),
        None if spec.get("url").is_some() => "http".into(),
        None => "stdio".into(),
    }
}

pub fn validate_spec(spec: &Value) -> Result<(), String> {
    let obj = spec.as_object().ok_or("服务器配置必须是 JSON 对象")?;
    let string_map = |key: &str| -> Result<(), String> {
        match obj.get(key) {
            None => Ok(()),
            Some(Value::Object(m)) if m.values().all(Value::is_string) => Ok(()),
            Some(_) => Err(format!("{} 必须是“名称: 字符串值”的对象", key)),
        }
    };
    match spec_transport(spec).as_str() {
        "stdio" => {
            if obj.get("command").and_then(Value::as_str).map(str::trim).unwrap_or("").is_empty() {
                return Err("stdio 服务器必须填写 command".into());
            }
            match obj.get("args") {
                None => {}
                Some(Value::Array(a)) if a.iter().all(Value::is_string) => {}
                Some(_) => return Err("args 必须是字符串数组".into()),
            }
            string_map("env")
        }
        "http" | "sse" => {
            let url = obj.get("url").and_then(Value::as_str).unwrap_or("").trim();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err("http / sse 服务器必须填写以 http:// 或 https:// 开头的 url".into());
            }
            string_map("headers")
        }
        other => Err(format!("不支持的类型 {}，只能是 stdio、http 或 sse", other)),
    }
}

/// 在保留位置的前提下替换键（同名时原地替换，改名时移到末尾）。
fn replace_key(map: &mut Map<String, Value>, original: &str, name: &str, value: Value) {
    if original != name {
        map.shift_remove(original);
    }
    map.insert(name.to_string(), value);
}

fn claude_entry(name: &str, spec: &Value, enabled: bool) -> McpServerEntry {
    McpServerEntry {
        client: McpClient::Claude,
        name: name.to_string(),
        transport: spec_transport(spec),
        spec: spec.clone(),
        enabled,
        read_only: false,
        cc_switch: None,
        raw: serde_json::to_string_pretty(&json!({ name: spec })).unwrap_or_default(),
    }
}

fn codex_entry(name: &str, item: &Item) -> Option<McpServerEntry> {
    let table = item.as_table_like()?;
    let mut spec = Map::new();
    let transport = match table.get("type").and_then(Item::as_str) {
        Some(t) => t.to_string(),
        None if table.contains_key("url") => "http".into(),
        None => "stdio".into(),
    };
    spec.insert("type".into(), Value::String(transport.clone()));
    let mut enabled = true;
    for (key, value) in table.iter() {
        match key {
            "type" => {}
            "enabled" => enabled = value.as_bool().unwrap_or(true),
            "http_headers" => {
                spec.insert("headers".into(), item_to_json(value));
            }
            _ => {
                spec.insert(key.to_string(), item_to_json(value));
            }
        }
    }
    let mut parent = Table::new();
    parent.set_implicit(true);
    parent.insert(name, item.clone());
    let mut doc = DocumentMut::new();
    doc.insert("mcp_servers", Item::Table(parent));
    Some(McpServerEntry {
        client: McpClient::Codex,
        name: name.to_string(),
        transport,
        spec: Value::Object(spec),
        enabled,
        read_only: CODEX_APP_MANAGED.contains(&name),
        cc_switch: None,
        raw: doc.to_string().trim().to_string(),
    })
}

/// 复制到 Claude：只保留 Claude Code 认识的字段，返回被舍弃的字段名。
fn spec_for_claude(spec: &Value) -> (Value, Vec<String>) {
    let transport = spec_transport(spec);
    let allowed = if transport == "stdio" { CLAUDE_STDIO_FIELDS } else { CLAUDE_REMOTE_FIELDS };
    let mut out = Map::new();
    out.insert("type".into(), Value::String(transport));
    let mut dropped = Vec::new();
    for (key, value) in spec.as_object().into_iter().flatten() {
        if key == "type" {
            continue;
        }
        if allowed.contains(&key.as_str()) {
            out.insert(key.clone(), value.clone());
        } else {
            dropped.push(key.clone());
        }
    }
    (Value::Object(out), dropped)
}

/// JSON 写法转 Codex 的表：不写 type，headers 写作 http_headers，舍弃另一种传输方式的字段。
/// `strict` 为真（跨客户端复制）时只保留 Codex 认识的字段。返回表和被舍弃的字段名。
fn spec_to_codex_table(spec: &Value, strict: bool) -> Result<(Table, Vec<String>), String> {
    let obj = spec.as_object().ok_or("服务器配置必须是 JSON 对象")?;
    let stdio = spec_transport(spec) == "stdio";
    let (own, other) = if stdio {
        (CODEX_STDIO_FIELDS, CODEX_REMOTE_FIELDS)
    } else {
        (CODEX_REMOTE_FIELDS, CODEX_STDIO_FIELDS)
    };
    let mut table = Table::new();
    let mut dropped = Vec::new();
    for (key, value) in obj {
        let key = key.as_str();
        if key == "type" || key == "enabled" {
            continue;
        }
        let known = own.contains(&key) || CODEX_COMMON_FIELDS.contains(&key);
        if other.contains(&key) || key == "http_headers" && stdio || (strict && !known) {
            dropped.push(key.to_string());
            continue;
        }
        let toml_key = if key == "headers" { "http_headers" } else { key };
        match (key, value) {
            // env 和 headers 写成子表，与 Codex 自己写出的格式一致。
            ("env" | "headers" | "http_headers", Value::Object(map)) => {
                let mut sub = Table::new();
                for (k, v) in map {
                    if let Some(v) = json_to_toml(v) {
                        sub.insert(k, Item::Value(v));
                    }
                }
                if !sub.is_empty() {
                    table.insert(toml_key, Item::Table(sub));
                }
            }
            _ => {
                if let Some(v) = json_to_toml(value) {
                    table.insert(toml_key, Item::Value(v));
                }
            }
        }
    }
    Ok((table, dropped))
}

fn json_to_toml(value: &Value) -> Option<toml_edit::Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items.iter().filter_map(json_to_toml) {
                array.push(item);
            }
            toml_edit::Value::Array(array)
        }
        Value::Object(map) => {
            let mut inline = InlineTable::new();
            for (k, v) in map {
                if let Some(v) = json_to_toml(v) {
                    inline.insert(k, v);
                }
            }
            toml_edit::Value::InlineTable(inline)
        }
    })
}

fn item_to_json(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(v) => toml_value_to_json(v),
        Item::Table(t) => Value::Object(t.iter().map(|(k, v)| (k.to_string(), item_to_json(v))).collect()),
        Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| Value::Object(t.iter().map(|(k, v)| (k.to_string(), item_to_json(v))).collect()))
                .collect(),
        ),
    }
}

fn toml_value_to_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => json!(*i.value()),
        toml_edit::Value::Float(f) => serde_json::Number::from_f64(*f.value())
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(f.value().to_string())),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(toml_value_to_json).collect()),
        toml_edit::Value::InlineTable(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.to_string(), toml_value_to_json(v))).collect())
        }
    }
}

/// `mcp_servers` 表；不存在时新建为隐式表，避免多出一行空的 `[mcp_servers]`。
fn codex_servers_mut(doc: &mut DocumentMut) -> Result<&mut dyn toml_edit::TableLike, String> {
    let item = doc.as_table_mut().entry("mcp_servers").or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    item.as_table_like_mut()
        .ok_or_else(|| "mcp_servers 不是表，请手动检查 config.toml".to_string())
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    parse_json_object(&bytes, path)
}

fn parse_json_object(bytes: &[u8], path: &Path) -> Result<Map<String, Value>, String> {
    let text = String::from_utf8_lossy(bytes);
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{} 的内容不是 JSON 对象", path.display())),
        Err(e) => Err(format!("{} 不是有效的 JSON：{}", path.display(), e)),
    }
}

fn read_toml(path: &Path) -> Result<DocumentMut, String> {
    let bytes = fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    parse_toml(&bytes, path)
}

fn parse_toml(bytes: &[u8], path: &Path) -> Result<DocumentMut, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{} 不是 UTF-8 文本", path.display()))?;
    text.trim_start_matches('\u{feff}')
        .parse::<DocumentMut>()
        .map_err(|e| format!("{} 不是有效的 TOML：{}", path.display(), e))
}

type Stamp = (SystemTime, u64);

fn file_stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn read_with_stamp(path: &Path) -> Result<(Vec<u8>, Stamp), String> {
    let stamp = file_stamp(path).ok_or_else(|| format!("无法读取 {}", path.display()))?;
    let bytes = fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    Ok((bytes, stamp))
}

/// 写入同目录的临时文件再替换目标；目标是符号链接时直接写入链接指向的文件。
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return fs::write(path, bytes).map_err(|e| format!("写入 {} 失败：{}", path.display(), e));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{}.cv-{}.tmp", name, uuid::Uuid::new_v4()));
    let result = fs::File::create(&tmp)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|_| fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!("写入 {} 失败：{}", path.display(), e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        root: PathBuf,
        manager: McpManager,
    }

    impl Sandbox {
        fn new(claude_json: &str, codex_toml: Option<&str>) -> Self {
            let root = std::env::temp_dir().join(format!("cv-mcp-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("codex")).unwrap();
            fs::write(root.join(".claude.json"), claude_json).unwrap();
            if let Some(toml) = codex_toml {
                fs::write(root.join("codex/config.toml"), toml).unwrap();
            }
            let manager = McpManager::new(&root.join(".claude.json"), &root.join("codex"), &root.join("data"), &root.join("backups"));
            Self { root, manager }
        }
        fn claude(&self) -> Value {
            serde_json::from_slice(&fs::read(self.root.join(".claude.json")).unwrap()).unwrap()
        }
        fn codex(&self) -> String {
            fs::read_to_string(self.root.join("codex/config.toml")).unwrap()
        }
        fn servers(&self, client: McpClient) -> Vec<McpServerEntry> {
            self.manager.list(&CcSwitchState::default()).into_iter().find(|l| l.client == client).unwrap().servers
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    const CLAUDE: &str = r#"{
  "numStartups": 3,
  "zeta": {"keep": true},
  "mcpServers": {
    "pencil": {"command": "pencil.exe", "args": ["--mcp"], "env": {}, "type": "stdio"},
    "remote": {"type": "http", "url": "https://example.com/mcp", "headers": {"Authorization": "Bearer x"}}
  },
  "alpha": 1
}"#;

    const CODEX: &str = r#"# 用户的注释
model = "gpt-5"

[mcp_servers.node_repl]
command = "node"
args = ["repl.js"]

[mcp_servers.node_repl.env]
CODEX_HOME = "C:\\x"

[mcp_servers.pencil]
command = "pencil.exe"
args = ["--mcp"]

[windows]
sandbox = "elevated"
"#;

    #[test]
    fn lists_both_clients_with_transport_and_read_only_entries() {
        let s = Sandbox::new(CLAUDE, Some(CODEX));
        let claude = s.servers(McpClient::Claude);
        assert_eq!(claude.iter().map(|e| (e.name.as_str(), e.transport.as_str())).collect::<Vec<_>>(),
                   vec![("pencil", "stdio"), ("remote", "http")]);
        let codex = s.servers(McpClient::Codex);
        assert_eq!(codex.len(), 2);
        let repl = codex.iter().find(|e| e.name == "node_repl").unwrap();
        assert!(repl.read_only);
        assert_eq!(repl.spec["env"]["CODEX_HOME"], "C:\\x");
        assert!(repl.raw.contains("[mcp_servers.node_repl.env]"));
        assert!(s.manager.delete(McpClient::Codex, "node_repl").is_err());
    }

    #[test]
    fn claude_disable_enable_keeps_other_fields_order_and_spec() {
        let s = Sandbox::new(CLAUDE, None);
        s.manager.set_enabled(McpClient::Claude, "pencil", false).unwrap();
        let root = s.claude();
        assert!(root["mcpServers"].get("pencil").is_none());
        assert_eq!(root.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["numStartups", "zeta", "mcpServers", "alpha"]);
        let listed = s.servers(McpClient::Claude);
        assert!(!listed.iter().find(|e| e.name == "pencil").unwrap().enabled);
        // 停用的条目不能重复停用，也能被编辑。
        assert!(s.manager.set_enabled(McpClient::Claude, "pencil", false).is_err());
        s.manager.upsert(McpClient::Claude, Some("pencil"), "pencil",
            &json!({"type": "stdio", "command": "pencil.exe", "args": ["--mcp", "--v2"]})).unwrap();
        s.manager.set_enabled(McpClient::Claude, "pencil", true).unwrap();
        assert_eq!(s.claude()["mcpServers"]["pencil"]["args"], json!(["--mcp", "--v2"]));
        assert!(s.servers(McpClient::Claude).iter().all(|e| e.enabled));
        assert!(fs::read_dir(s.root.join("backups/mcp")).unwrap().count() >= 2);
    }

    #[test]
    fn claude_create_rename_and_delete() {
        let s = Sandbox::new(CLAUDE, None);
        let spec = json!({"type": "stdio", "command": "uvx", "args": ["tool"], "env": {"K": "V"}});
        s.manager.upsert(McpClient::Claude, None, "tool", &spec).unwrap();
        assert!(s.manager.upsert(McpClient::Claude, None, "tool", &spec).is_err());
        assert!(s.manager.upsert(McpClient::Claude, None, "bad name", &spec).is_err());
        assert!(s.manager.upsert(McpClient::Claude, None, "x", &json!({"type": "stdio"})).is_err());
        assert!(s.manager.upsert(McpClient::Claude, None, "x", &json!({"type": "http", "url": "ftp://a"})).is_err());
        s.manager.upsert(McpClient::Claude, Some("pencil"), "pencil2", &json!({"command": "p.exe"})).unwrap();
        let keys: Vec<String> = s.claude()["mcpServers"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, vec!["remote", "tool", "pencil2"]);
        s.manager.delete(McpClient::Claude, "tool").unwrap();
        assert!(s.manager.delete(McpClient::Claude, "tool").is_err());
        assert_eq!(s.claude()["numStartups"], 3);
    }

    #[test]
    fn codex_enable_flag_edit_and_comments_survive() {
        let s = Sandbox::new(CLAUDE, Some(CODEX));
        s.manager.set_enabled(McpClient::Codex, "pencil", false).unwrap();
        assert!(s.codex().contains("enabled = false"));
        assert!(!s.servers(McpClient::Codex).iter().find(|e| e.name == "pencil").unwrap().enabled);
        // 编辑保持停用状态和位置。
        s.manager.upsert(McpClient::Codex, Some("pencil"), "pencil",
            &json!({"type": "stdio", "command": "pencil.exe", "args": ["--mcp"], "env": {"A": "1"}})).unwrap();
        let text = s.codex();
        assert!(text.starts_with("# 用户的注释\nmodel = \"gpt-5\""));
        assert!(text.contains("enabled = false"));
        assert!(text.contains("[mcp_servers.pencil.env]\nA = \"1\""));
        assert!(text.find("[mcp_servers.pencil]").unwrap() < text.find("[windows]").unwrap());
        s.manager.set_enabled(McpClient::Codex, "pencil", true).unwrap();
        assert!(!s.codex().contains("enabled"));
        s.manager.delete(McpClient::Codex, "pencil").unwrap();
        let text = s.codex();
        assert!(!text.contains("pencil") && text.contains("[mcp_servers.node_repl.env]") && text.contains("[windows]"));
        assert_eq!(text.parse::<DocumentMut>().unwrap()["windows"]["sandbox"].as_str(), Some("elevated"));
    }

    #[test]
    fn copies_between_clients_with_format_conversion() {
        let s = Sandbox::new(CLAUDE, Some(CODEX));
        assert!(s.manager.copy(McpClient::Claude, "pencil", McpClient::Codex, false).is_err());
        let out = s.manager.copy(McpClient::Claude, "remote", McpClient::Codex, false).unwrap();
        assert!(out.dropped_fields.is_empty());
        let doc = s.codex().parse::<DocumentMut>().unwrap();
        let remote = &doc["mcp_servers"]["remote"];
        assert_eq!(remote["url"].as_str(), Some("https://example.com/mcp"));
        assert_eq!(remote["http_headers"]["Authorization"].as_str(), Some("Bearer x"));
        assert!(remote.get("type").is_none() && remote.get("headers").is_none());

        // Codex → Claude：舍弃 Claude 不认识的字段。
        s.manager.upsert(McpClient::Codex, None, "fluent",
            &json!({"type": "stdio", "command": "py.exe", "args": ["-m", "mcp"], "env": {"K": "V"}, "startup_timeout_sec": 30, "cwd": "E:\\w"})).unwrap();
        let out = s.manager.copy(McpClient::Codex, "fluent", McpClient::Claude, false).unwrap();
        assert_eq!(out.dropped_fields, vec!["startup_timeout_sec".to_string(), "cwd".to_string()]);
        assert_eq!(s.claude()["mcpServers"]["fluent"], json!({"type": "stdio", "command": "py.exe", "args": ["-m", "mcp"], "env": {"K": "V"}}));
        assert!(s.manager.copy(McpClient::Codex, "fluent", McpClient::Claude, false).is_err());
        s.manager.copy(McpClient::Codex, "fluent", McpClient::Claude, true).unwrap();
        assert!(s.manager.copy(McpClient::Codex, "node_repl", McpClient::Claude, false).is_err());
    }

    #[test]
    fn creates_codex_config_and_rejects_cross_transport_fields() {
        let s = Sandbox::new(CLAUDE, None);
        s.manager.upsert(McpClient::Codex, None, "web",
            &json!({"type": "http", "url": "https://a/mcp", "command": "stray", "headers": {"H": "v"}})).unwrap();
        let text = s.codex();
        assert!(!text.contains("[mcp_servers]\n"), "{text}");
        assert!(!text.contains("stray"));
        assert!(text.contains("[mcp_servers.web]\nurl = \"https://a/mcp\""));
        let web = &s.servers(McpClient::Codex)[0];
        assert_eq!(web.transport, "http");
        assert_eq!(web.spec["headers"]["H"], "v");
    }

    #[test]
    fn malformed_files_are_reported_and_left_untouched() {
        let s = Sandbox::new("{ not json", Some("mcp_servers = 3\n"));
        let locations = s.manager.list(&CcSwitchState::default());
        assert!(locations.iter().all(|l| l.error.is_some()));
        assert!(s.manager.upsert(McpClient::Claude, None, "a", &json!({"command": "x"})).is_err());
        assert!(s.manager.upsert(McpClient::Codex, None, "a", &json!({"command": "x"})).is_err());
        assert_eq!(fs::read_to_string(s.root.join(".claude.json")).unwrap(), "{ not json");
        assert_eq!(s.codex(), "mcp_servers = 3\n");
    }
}
