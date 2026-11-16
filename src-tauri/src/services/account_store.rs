//! 各客户端账号管理共用的类型和工具：发给界面的账号视图、额度、备份、原子写入、JWT / base64 解析。
//!
//! Codex、Claude、Antigravity 的账号分别保存在各自的文件里，切换时只改对应客户端的登录文件，互不影响。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{Local, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const MAX_BACKUPS: usize = 30;
const EDIT_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountClient {
    Codex,
    Claude,
    Antigravity,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    /// Codex：`primary` / `secondary`；Claude：`five_hour`、`seven_day` 等；Antigravity：模型 id。
    pub kind: String,
    /// 显示名（如模型名）；为空时界面按窗口长度命名。
    #[serde(default)]
    pub label: Option<String>,
    pub used_percent: f64,
    #[serde(default)]
    pub window_minutes: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Quota {
    pub plan_type: Option<String>,
    pub windows: Vec<QuotaWindow>,
    /// 额外点数 / 余额（有的套餐才有）。
    #[serde(default)]
    pub credits_balance: Option<String>,
}

/// 账号上的标签与备注元数据（按账号持久化，导出与 Cockpit 互通时保留）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountMeta {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

pub fn normalize_tags(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.iter().any(|existing| existing.eq_ignore_ascii_case(trimmed)) {
            out.push(trimmed.to_string());
        }
    }
    out
}

pub fn normalize_note(note: Option<String>) -> Option<String> {
    note.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl AccountMeta {
    pub fn normalized(tags: impl IntoIterator<Item = String>, note: Option<String>) -> Self {
        Self {
            tags: normalize_tags(tags),
            note: normalize_note(note),
        }
    }

    pub fn update(&mut self, tags: Option<Vec<String>>, note: Option<Option<String>>) {
        if let Some(t) = tags {
            self.tags = normalize_tags(t);
        }
        if let Some(n) = note {
            self.note = normalize_note(n);
        }
    }

    pub fn add_tags(&mut self, tags: impl IntoIterator<Item = String>) {
        let mut combined = std::mem::take(&mut self.tags);
        combined.extend(tags);
        self.tags = normalize_tags(combined);
    }

    pub fn remove_tags(&mut self, tags: &[String]) {
        let targets: Vec<String> = tags
            .iter()
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        self.tags
            .retain(|existing| !targets.iter().any(|target| existing.eq_ignore_ascii_case(target)));
    }
}

/// 保存在账号上的最近一次额度查询结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaState {
    #[serde(default)]
    pub quota: Option<Quota>,
    #[serde(default)]
    pub quota_error: Option<String>,
    #[serde(default)]
    pub quota_updated_at: Option<String>,
    #[serde(default)]
    pub verification_url: Option<String>,
}

impl QuotaState {
    pub fn record(&mut self, result: &Result<Quota, String>) {
        self.record_with_verification(result, None);
    }

    pub fn record_with_verification(
        &mut self,
        result: &Result<Quota, String>,
        verification_url: Option<String>,
    ) {
        match result {
            Ok(quota) => {
                self.quota = Some(quota.clone());
                self.quota_error = None;
                self.verification_url = None;
            }
            Err(message) => {
                self.quota_error = Some(message.clone());
                self.verification_url = verification_url.or_else(|| self.verification_url.take());
            }
        }
        self.quota_updated_at = Some(Utc::now().to_rfc3339());
    }
}

/// 发给界面的账号，不含任何 token。
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountView {
    pub id: String,
    /// Claude/Codex：`oauth`（官方登录）或 `provider`（中转供应商）；Antigravity 为空。
    pub kind: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
    /// 补充说明，如中转供应商的地址和模型。
    pub detail: Option<String>,
    pub is_current: bool,
    pub access_expires_at: Option<String>,
    pub last_refresh: Option<String>,
    pub added_at: String,
    pub last_used_at: Option<String>,
    /// 能否查询额度（中转供应商不能）。
    pub has_quota: bool,
    pub quota: Option<Quota>,
    pub quota_error: Option<String>,
    pub quota_updated_at: Option<String>,
    pub verification_url: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub auth_field: Option<String>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
}

impl AccountView {
    pub fn with_quota(mut self, state: &QuotaState) -> Self {
        self.quota = state.quota.clone();
        self.quota_error = state.quota_error.clone();
        self.quota_updated_at = state.quota_updated_at.clone();
        self.verification_url = state.verification_url.clone();
        self
    }

    pub fn with_meta(mut self, meta: &AccountMeta) -> Self {
        self.tags = meta.tags.clone();
        self.note = meta.note.clone();
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CurrentLogin {
    pub kind: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan_type: Option<String>,
    /// 当前登录已保存为某个账号。
    pub saved: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountsState {
    /// 该客户端的登录文件位置。
    pub auth_path: String,
    pub current: Option<CurrentLogin>,
    /// 当前不能切换的原因（如凭据在系统钥匙串里、客户端未安装）。
    pub current_note: Option<String>,
    /// 其他提示（如中转供应商盖过了官方登录）。
    pub notes: Vec<String>,
    pub accounts: Vec<AccountView>,
    pub cockpit_import_count: usize,
    pub cc_switch_available: bool,
}

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 读取 JSON 文件；不存在时返回默认值。
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    if !path.is_file() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{} 已损坏：{}", path.display(), e))
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{}", dir.display(), e))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    atomic_write(path, text.as_bytes())
}

/// 写入同目录的临时文件再替换目标，避免写到一半损坏。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

/// 把文件内容备份为 `<dir>/<prefix>-<时间>.bak`，同前缀只保留最近 `keep` 份。
pub fn backup(dir: &Path, prefix: &str, bytes: &[u8], keep: usize) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("无法创建备份目录 {}：{}", dir.display(), e))?;
    let file = dir.join(format!("{}-{}.bak", prefix, Local::now().format("%Y%m%d-%H%M%S-%3f")));
    fs::write(&file, bytes).map_err(|e| format!("备份 {} 失败：{}", prefix, e))?;
    let mut existing: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{}-", prefix))))
                .collect()
        })
        .unwrap_or_default();
    existing.sort();
    let excess = existing.len().saturating_sub(keep.max(1));
    for old in existing.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
    Ok(file)
}

/// 备份一个文件（存在时）；保留最近 30 份。
pub fn backup_file(dir: &Path, path: &Path) -> Result<(), String> {
    if let Ok(bytes) = fs::read(path) {
        let prefix = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        backup(dir, &prefix, &bytes, MAX_BACKUPS)?;
    }
    Ok(())
}

type Stamp = (SystemTime, u64);

fn file_stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// 修改一个 JSON 对象文件：读取 → `edit` → 写回，期间文件被其他程序改过就重来（Claude Code
/// 运行时会频繁改写 `.claude.json`）。保留其余字段和键顺序；写前备份到 `backup_dir`。
/// `create` 为真时文件不存在就从空对象开始。
pub fn edit_json_file<F>(path: &Path, backup_dir: &Path, create: bool, mut edit: F) -> Result<(), String>
where
    F: FnMut(&mut Map<String, Value>) -> Result<(), String>,
{
    for _ in 0..EDIT_ATTEMPTS {
        let stamp = file_stamp(path);
        let bytes = match stamp {
            Some(_) => fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?,
            None if create => Vec::new(),
            None => return Err(format!("没有找到 {}", path.display())),
        };
        let mut root = if bytes.is_empty() {
            Map::new()
        } else {
            let text = String::from_utf8_lossy(&bytes);
            match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
                Ok(Value::Object(map)) => map,
                _ => return Err(format!("{} 不是有效的 JSON 对象，请手动检查", path.display())),
            }
        };
        edit(&mut root)?;
        let mut text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
        if bytes.ends_with(b"\n") {
            text.push('\n');
        }
        if file_stamp(path) != stamp {
            continue;
        }
        if !bytes.is_empty() {
            let prefix = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            backup(backup_dir, &prefix, &bytes, MAX_BACKUPS)?;
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{}", dir.display(), e))?;
        }
        if atomic_write(path, text.as_bytes()).is_ok() {
            return Ok(());
        }
    }
    Err(format!("{} 正被其他程序频繁改写，请稍后再试", path.display()))
}

/// 读取 JWT 的载荷（不验证签名，只用来显示身份和过期时间）。
pub fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&base64_decode(payload)?).ok()
}

/// 解码 base64（同时接受标准和 URL 安全字母表，忽略填充）。
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in input.trim().bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32;
        buffer = (buffer << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// URL 安全 Base64 编码（无填充），适用于 PKCE / JWT / OAuth state。
pub fn base64_url_encode(data: &[u8]) -> String {
    base64_encode(data)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// 生成 PKCE `(verifier, challenge)`（S256）。
pub fn pkce_pair() -> (String, String) {
    use sha2::{Digest, Sha256};
    let verifier = format!(
        "{}{}",
        base64_url_encode(uuid::Uuid::new_v4().as_bytes()),
        base64_url_encode(uuid::Uuid::new_v4().as_bytes()),
    );
    let challenge = base64_url_encode(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// 解析 URL 百分号编码（`%XX`）。
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 从完整回调 URL、`code#state` 或 query 参数中提取 OAuth `code`。
pub fn extract_callback_code(raw: &str, expected_state: Option<&str>) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !trimmed.contains("://") && !trimmed.contains('?') && !trimmed.contains("code=") {
        if let Some((code, state)) = trimmed.split_once('#') {
            if let Some(exp) = expected_state {
                if !state.is_empty() && state != exp {
                    return Err("回调 state 不匹配，请重新发起授权".into());
                }
            }
            let code = percent_decode(code.trim());
            return Ok((!code.is_empty()).then_some(code));
        }
    }
    let query = trimmed
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or(trimmed)
        .split('#')
        .next()
        .unwrap_or("");
    let mut code = None;
    let mut state = None;
    let mut err = None;
    let mut err_desc = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        if let Some((k, v)) = pair.split_once('=') {
            let val = percent_decode(v);
            match k {
                "code" => code = Some(val),
                "state" => state = Some(val),
                "error" => err = Some(val),
                "error_description" => err_desc = Some(val),
                _ => {}
            }
        }
    }
    if let Some(e) = err {
        let msg = err_desc.unwrap_or(e);
        return Err(format!("授权失败：{}", msg));
    }
    if let (Some(exp), Some(got)) = (expected_state, state.as_deref()) {
        if !got.is_empty() && got != exp {
            return Err("回调 state 不匹配，请重新发起授权".into());
        }
    }
    Ok(code.filter(|c| !c.is_empty()))
}

/// OAuth 本机回调成功/失败展示页。
pub fn oauth_callback_page(title: &str, message: &str) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{t}</title>\
         <body style=\"font-family:'Microsoft YaHei UI','Segoe UI',sans-serif;background:#f5f5f7;display:grid;place-items:center;height:100vh;margin:0\">\
         <div style=\"background:#fff;padding:32px 40px;border-radius:16px;box-shadow:0 8px 30px rgba(0,0,0,.08);text-align:center\">\
         <h2 style=\"margin:0 0 8px\">{t}</h2><p style=\"margin:0;color:#6e6e73\">{m}</p></div></body>",
        t = escape(title),
        m = escape(message)
    )
}

/// 用默认浏览器打开授权链接。
pub fn open_in_browser(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// 返回 Cockpit Tools 账号数据目录候选列表（`~/.cockpit_tools` 等）。
pub fn cockpit_data_dirs() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".cockpit_tools"));
    }
    if let Some(data_dir) = dirs::data_dir() {
        let p = data_dir.join(".cockpit_tools");
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// 标准 base64 编码（带填充）。
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `application/x-www-form-urlencoded` 编码。
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{:02X}", b),
            })
            .collect::<String>()
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// 从 HTTP 错误响应里取 `error` / `error_description` / `message`，便于提示。
pub fn error_code(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            let e = v.get("error")?;
            Some(match e {
                Value::String(s) => s.clone(),
                other => other.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| other.to_string()),
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip_and_form_encoding() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"hello world!"] {
            assert_eq!(base64_decode(&base64_encode(data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(base64_url_encode(b"\xfb\xff\xef"), "-__v");
        assert_eq!(form_encode(&[("refresh_token", "1//a b+c"), ("x", "y")]), "refresh_token=1%2F%2Fa%20b%2Bc&x=y");
    }

    #[test]
    fn account_meta_normalizes_tags_notes_and_verification() {
        let mut meta = AccountMeta::normalized(
            vec!["  work ".into(), "".into(), "WORK".into(), "pro".into()],
            Some("  主力账号  ".into()),
        );
        assert_eq!(meta.tags, vec!["work", "pro"]);
        assert_eq!(meta.note.as_deref(), Some("主力账号"));
        meta.add_tags(vec!["Pro".into(), "backup".into()]);
        assert_eq!(meta.tags, vec!["work", "pro", "backup"]);
        meta.remove_tags(&["WORK".into()]);
        assert_eq!(meta.tags, vec!["pro", "backup"]);
        meta.update(None, Some(Some("   ".into())));
        assert_eq!(meta.note, None);

        let mut qs = QuotaState::default();
        qs.record_with_verification(&Err("请完成验证".into()), Some("https://accounts.google.com/v".into()));
        assert_eq!(qs.verification_url.as_deref(), Some("https://accounts.google.com/v"));
        qs.record(&Ok(Quota::default()));
        assert_eq!(qs.verification_url, None);
        assert_eq!(qs.quota_error, None);
    }

    #[test]
    fn extracts_oauth_callback_codes_and_pkce() {
        let (v, c) = pkce_pair();
        assert!(v.len() >= 43 && !v.contains('=') && !c.contains('='));
        assert_eq!(
            extract_callback_code("http://127.0.0.1:1455/auth/callback?code=abc%2F1&state=st1", Some("st1")).unwrap(),
            Some("abc/1".into())
        );
        assert_eq!(
            extract_callback_code("cl-code#st2", Some("st2")).unwrap(),
            Some("cl-code".into())
        );
        assert!(extract_callback_code("http://localhost:1455/auth/callback?code=x&state=wrong", Some("st1")).is_err());
        assert!(extract_callback_code("http://localhost/cb?error=access_denied", None).is_err());
    }

    #[test]
    fn edit_json_file_keeps_other_fields_and_backs_up() {
        let dir = std::env::temp_dir().join(format!("cv-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        fs::write(&file, "{\n  \"b\": 1,\n  \"a\": {\"x\": true}\n}\n").unwrap();
        edit_json_file(&file, &dir.join("bak"), false, |root| {
            root.insert("env".into(), serde_json::json!({ "K": "V" }));
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("{\n  \"b\": 1,\n  \"a\"") && text.ends_with("}\n"));
        assert_eq!(fs::read_dir(dir.join("bak")).unwrap().count(), 1);
        assert!(edit_json_file(&dir.join("missing.json"), &dir, false, |_| Ok(())).is_err());
        edit_json_file(&dir.join("new.json"), &dir, true, |root| {
            root.insert("a".into(), 1.into());
            Ok(())
        })
        .unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
