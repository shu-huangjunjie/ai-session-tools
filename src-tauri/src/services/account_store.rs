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

/// 保存在账号上的最近一次额度查询结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaState {
    #[serde(default)]
    pub quota: Option<Quota>,
    #[serde(default)]
    pub quota_error: Option<String>,
    #[serde(default)]
    pub quota_updated_at: Option<String>,
}

impl QuotaState {
    pub fn record(&mut self, result: &Result<Quota, String>) {
        match result {
            Ok(quota) => {
                self.quota = Some(quota.clone());
                self.quota_error = None;
            }
            Err(message) => self.quota_error = Some(message.clone()),
        }
        self.quota_updated_at = Some(Utc::now().to_rfc3339());
    }
}

/// 发给界面的账号，不含任何 token。
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountView {
    pub id: String,
    /// Claude：`oauth`（官方登录）或 `provider`（中转供应商）；其他客户端为空。
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
}

impl AccountView {
    pub fn with_quota(mut self, state: &QuotaState) -> Self {
        self.quota = state.quota.clone();
        self.quota_error = state.quota_error.clone();
        self.quota_updated_at = state.quota_updated_at.clone();
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
        assert_eq!(form_encode(&[("refresh_token", "1//a b+c"), ("x", "y")]), "refresh_token=1%2F%2Fa%20b%2Bc&x=y");
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
