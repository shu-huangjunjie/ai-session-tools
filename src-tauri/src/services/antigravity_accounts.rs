//! Antigravity 账号：保存、切换、删除，以及查询各模型的剩余额度。
//!
//! Antigravity 把登录保存在 `%APPDATA%\Antigravity\User\globalStorage\state.vscdb` 的 `ItemTable` 里。
//! 一个账号就是 [`KEYS`] 这几项的快照；切换时（Antigravity 必须已关闭，否则它会把内存里的登录写回）
//! 先备份数据库，再在一个事务里写回目标账号的这几项。
//!
//! 额度：用快照里的 Google refresh token 换 access token（OAuth 参数在运行时从已安装的
//! Antigravity `language_server.exe` 中读取；Google 刷新不会让旧 token 作废），再调用 Cloud Code 的
//! `loadCodeAssist` 取项目和套餐，`fetchAvailableModels` 取每个模型的剩余比例和重置时间。
//!
//! 添加账号也可以不经过 Antigravity：在浏览器里用 Google 授权，或导入 refresh token（见文件后半部分）。
//!
//! 账号保存在本程序数据目录的 `accounts/antigravity.json`，与 Codex、Claude 互不相干。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::account_store::{self, AccountView, AccountsState, CurrentLogin, Quota, QuotaState, QuotaWindow};
use super::win_http::{self, HttpFn};

/// 属于账号的 `ItemTable` 键。
pub const KEYS: &[&str] = &[
    "antigravityAuthStatus",
    "antigravityUnifiedStateSync.oauthToken",
    "antigravityUnifiedStateSync.userStatus",
    "antigravityUnifiedStateSync.enterprisePreferences",
    "antigravityUnifiedStateSync.modelCredits",
    "antigravity.profileUrl",
];
const AUTH_KEY: &str = "antigravityAuthStatus";
const TOKEN_KEY: &str = "antigravityUnifiedStateSync.oauthToken";
const TOKEN_SENTINEL: &str = "oauthTokenInfoSentinelKey";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CLOUD_CODE: &str = "https://cloudcode-pa.googleapis.com";
const USER_AGENT: &str = "antigravity/1.20.5 windows/amd64";
const STORE_FILE: &str = "antigravity.json";
const DB_BACKUPS: usize = 10;

/// `ItemTable` 中的一个值，保持原来的存储类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Item {
    Text(String),
    /// base64 编码的二进制。
    Blob(String),
}

impl Item {
    fn text(&self) -> Option<&str> {
        match self {
            Item::Text(s) => Some(s),
            Item::Blob(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    id: String,
    email: String,
    #[serde(default)]
    name: Option<String>,
    /// [`KEYS`] 中存在的项。
    items: Map<String, Value>,
    /// 登录项是按 token 合成的（浏览器授权或导入），还没有 Antigravity 写下的完整快照。
    #[serde(default)]
    synthetic: bool,
    /// 刷新成功时用的 OAuth client id。
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    access_expires_at: Option<i64>,
    #[serde(default)]
    project_id: Option<String>,
    added_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(flatten)]
    quota: QuotaState,
}

impl StoredAccount {
    fn item(&self, key: &str) -> Option<Item> {
        self.items.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: Vec<StoredAccount>,
}

/// 默认的 Antigravity 数据库；测试或便携使用时可用环境变量 `ANTIGRAVITY_STATE_DB` 指定。
pub fn default_state_db() -> PathBuf {
    if let Ok(path) = std::env::var("ANTIGRAVITY_STATE_DB") {
        return PathBuf::from(path);
    }
    let base = dirs::config_dir().unwrap_or_default();
    let candidates = ["Antigravity", "Antigravity IDE"].map(|d| base.join(d).join("User").join("globalStorage").join("state.vscdb"));
    candidates.iter().find(|p| p.is_file()).cloned().unwrap_or_else(|| candidates[0].clone())
}

pub struct AntigravityAccountManager {
    db: PathBuf,
    store_path: PathBuf,
    backups: PathBuf,
}

impl AntigravityAccountManager {
    pub fn new(db: &Path, data_dir: &Path, backups: &Path) -> Self {
        Self {
            db: db.to_path_buf(),
            store_path: data_dir.join("accounts").join(STORE_FILE),
            backups: backups.join("accounts").join("antigravity"),
        }
    }

    fn load(&self) -> Result<Store, String> {
        account_store::load_json(&self.store_path)
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        account_store::save_json(&self.store_path, &json!({ "version": 1, "accounts": store.accounts }))
    }

    /// 当前登录的快照；数据库不存在或没有登录时为空。
    fn read_live(&self) -> Result<Option<Map<String, Value>>, String> {
        if !self.db.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| format!("打开 Antigravity 数据库失败：{}", e))?;
        let mut items = Map::new();
        for key in KEYS {
            let value: Option<rusqlite::types::Value> = conn
                .query_row("SELECT value FROM ItemTable WHERE key = ?", [key], |r| r.get(0))
                .optional()
                .map_err(|e| format!("读取 Antigravity 数据库失败：{}", e))?;
            let item = match value {
                Some(rusqlite::types::Value::Text(s)) => Item::Text(s),
                Some(rusqlite::types::Value::Blob(b)) => Item::Blob(account_store::base64_encode(&b)),
                _ => continue,
            };
            items.insert(key.to_string(), serde_json::to_value(item).map_err(|e| e.to_string())?);
        }
        Ok(items.contains_key(AUTH_KEY).then_some(items))
    }

    pub fn state(&self) -> Result<AccountsState, String> {
        let store = self.load()?;
        let live = self.read_live().unwrap_or(None);
        let live_identity = live.as_ref().and_then(|items| identity(items.get(AUTH_KEY)));
        let live_email = live_identity.as_ref().map(|(e, _)| e.to_lowercase());
        let current = live_identity.map(|(email, name)| CurrentLogin {
            saved: store.accounts.iter().any(|a| a.email.to_lowercase() == email.to_lowercase()),
            email: Some(email),
            name,
            ..Default::default()
        });
        Ok(AccountsState {
            auth_path: self.db.to_string_lossy().to_string(),
            current,
            current_note: (!self.db.is_file()).then(|| format!("没有找到 Antigravity 的数据库：{}", self.db.display())),
            notes: Vec::new(),
            accounts: store.accounts.iter().map(|a| view(a, live_email.as_deref() == Some(a.email.to_lowercase().as_str()))).collect(),
        })
    }

    /// 保存当前登录（已保存则用 Antigravity 的最新数据覆盖），返回账号 id。
    pub fn save_current(&self) -> Result<String, String> {
        let items = self.read_live()?.ok_or("当前没有 Antigravity 登录")?;
        let (email, name) = identity(items.get(AUTH_KEY)).ok_or("Antigravity 的登录信息里没有邮箱")?;
        if token_info(&items).is_none() {
            return Err("Antigravity 的登录信息里没有 token，请在 Antigravity 中重新登录".into());
        }
        let mut store = self.load()?;
        let id = match store.accounts.iter_mut().find(|a| a.email.eq_ignore_ascii_case(&email)) {
            Some(existing) => {
                if existing.items != items {
                    existing.items = items;
                    existing.access_token = None;
                    existing.access_expires_at = None;
                }
                existing.synthetic = false;
                existing.name = name;
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    email,
                    name,
                    items,
                    synthetic: false,
                    client_id: None,
                    access_token: None,
                    access_expires_at: None,
                    project_id: None,
                    added_at: account_store::now(),
                    last_used_at: None,
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }

    /// 切换账号。调用方须先确认 Antigravity 已关闭。
    pub fn switch(&self, id: &str) -> Result<(), String> {
        self.switch_with(id, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn switch_with(&self, id: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<(), String> {
        if !self.db.is_file() {
            return Err(format!("没有找到 Antigravity 的数据库：{}", self.db.display()));
        }
        if self.read_live()?.is_some() {
            self.save_current()?;
        }
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        if target.synthetic {
            // 合成的登录项带的 access token 多半已过期：先换一个新的，换不到就让 Antigravity 自己用 refresh token 换。
            if let Some((_, refresh_token, _)) = token_info(&target.items) {
                if let Ok((client_id, access, expires_at)) = refresh(&refresh_token, target.client_id.as_deref(), http, clients) {
                    target.items = synth_items(&target.email, target.name.as_deref(), &access, &refresh_token, expires_at, target.project_id.as_deref());
                    target.client_id = Some(client_id);
                    target.access_token = Some(access);
                    target.access_expires_at = Some(expires_at);
                }
            }
        }
        let bytes = std::fs::read(&self.db).map_err(|e| format!("读取 Antigravity 数据库失败：{}", e))?;
        account_store::backup(&self.backups, "state.vscdb", &bytes, DB_BACKUPS)?;

        let mut conn = Connection::open(&self.db).map_err(|e| format!("打开 Antigravity 数据库失败：{}", e))?;
        let tx = conn.transaction().map_err(|e| format!("写入 Antigravity 数据库失败：{}", e))?;
        for key in KEYS {
            match target.item(key) {
                Some(Item::Text(s)) => tx.execute("INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)", rusqlite::params![key, s]),
                Some(Item::Blob(b)) => {
                    let blob = account_store::base64_decode(&b).ok_or("账号数据损坏")?;
                    tx.execute("INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)", rusqlite::params![key, blob])
                }
                None => tx.execute("DELETE FROM ItemTable WHERE key = ?1", [key]),
            }
            .map_err(|e| format!("写入 Antigravity 数据库失败：{}", e))?;
        }
        tx.execute("INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityOnboarding', 'true')", [])
            .map_err(|e| format!("写入 Antigravity 数据库失败：{}", e))?;
        tx.commit().map_err(|e| format!("写入 Antigravity 数据库失败：{}", e))?;
        target.last_used_at = Some(account_store::now());
        self.save(&store)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut store = self.load()?;
        let before = store.accounts.len();
        store.accounts.retain(|a| a.id != id);
        if store.accounts.len() == before {
            return Err("账号不存在".into());
        }
        self.save(&store)
    }

    pub fn refresh_quota(&self, id: &str) -> Result<(), String> {
        self.refresh_quota_with(id, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn refresh_quota_with(&self, id: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<(), String> {
        let store = self.load()?;
        let account = store.accounts.iter().find(|a| a.id == id).ok_or("账号不存在")?.clone();
        let mut updated = account.clone();
        let result = (|| -> Result<Quota, String> {
            let token = match (&account.access_token, account.access_expires_at) {
                (Some(t), Some(exp)) if exp - 60 > Utc::now().timestamp() => t.clone(),
                _ => {
                    let (_, refresh_token, _) = token_info(&account.items).ok_or("账号里没有 token，请在「添加账号」里重新授权这个账号")?;
                    let (client_id, access, expires_at) = refresh(&refresh_token, account.client_id.as_deref(), http, clients)?;
                    updated.client_id = Some(client_id);
                    updated.access_token = Some(access.clone());
                    updated.access_expires_at = Some(expires_at);
                    access
                }
            };
            let (project, plan) = load_code_assist(&token, http)?;
            if project.is_some() {
                updated.project_id = project.clone();
            }
            let mut quota = fetch_models(&token, project.or(account.project_id.clone()), http)?;
            quota.plan_type = plan;
            Ok(quota)
        })();

        let mut store = self.load()?;
        let Some(stored) = store.accounts.iter_mut().find(|a| a.id == id) else { return Ok(()) };
        stored.client_id = updated.client_id;
        stored.access_token = updated.access_token;
        stored.access_expires_at = updated.access_expires_at;
        stored.project_id = updated.project_id;
        stored.quota.record(&result);
        self.save(&store)?;
        result.map(|_| ())
    }
}

/// `antigravityAuthStatus` 里的邮箱和名字。
fn identity(auth: Option<&Value>) -> Option<(String, Option<String>)> {
    let item: Item = serde_json::from_value(auth?.clone()).ok()?;
    let value: Value = serde_json::from_str(item.text()?).ok()?;
    let email = value.get("email").and_then(Value::as_str).filter(|e| !e.is_empty())?.to_string();
    Some((email, value.get("name").and_then(Value::as_str).map(str::to_string)))
}

/// 从 `oauthToken` 的 protobuf 中取出 `(access_token, refresh_token, 过期时间)`。
fn token_info(items: &Map<String, Value>) -> Option<(String, String, Option<i64>)> {
    let item: Item = serde_json::from_value(items.get(TOKEN_KEY)?.clone()).ok()?;
    let raw = account_store::base64_decode(item.text()?)?;
    for (_, entry) in fields(&raw)?.into_iter().filter(|(f, _)| *f == 1) {
        let Field::Bytes(entry) = entry else { continue };
        let sub = fields(entry)?;
        let key = sub.iter().find_map(|(f, v)| match (f, v) {
            (1, Field::Bytes(b)) => std::str::from_utf8(b).ok(),
            _ => None,
        });
        if key != Some(TOKEN_SENTINEL) {
            continue;
        }
        let Some((_, Field::Bytes(wrapper))) = sub.iter().find(|(f, _)| *f == 2) else { continue };
        let encoded = fields(wrapper)?.into_iter().find_map(|(f, v)| match (f, v) {
            (1, Field::Bytes(b)) => std::str::from_utf8(b).ok().map(str::to_string),
            _ => None,
        })?;
        let info = account_store::base64_decode(&encoded)?;
        let info = fields(&info)?;
        let text = |n: u32| {
            info.iter().find_map(|(f, v)| match v {
                Field::Bytes(b) if *f == n => std::str::from_utf8(b).ok().map(str::to_string),
                _ => None,
            })
        };
        let expiry = info.iter().find_map(|(f, v)| match v {
            Field::Bytes(b) if *f == 4 => fields(b)?.into_iter().find_map(|(f, v)| match (f, v) {
                (1, Field::Varint(n)) => Some(n as i64),
                _ => None,
            }),
            _ => None,
        });
        return Some((text(1).unwrap_or_default(), text(3).filter(|s| !s.is_empty())?, expiry));
    }
    None
}

enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

/// 解析一层 protobuf 字段；格式不对时返回空。
fn fields(data: &[u8]) -> Option<Vec<(u32, Field<'_>)>> {
    fn varint(data: &[u8], i: &mut usize) -> Option<u64> {
        let mut n = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *data.get(*i)?;
            *i += 1;
            n |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return Some(n);
            }
        }
        None
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let key = varint(data, &mut i)?;
        let field = (key >> 3) as u32;
        match key & 7 {
            0 => out.push((field, Field::Varint(varint(data, &mut i)?))),
            1 => i += 8,
            2 => {
                let len = varint(data, &mut i)? as usize;
                out.push((field, Field::Bytes(data.get(i..i + len)?)));
                i += len;
            }
            5 => i += 4,
            _ => return None,
        }
    }
    (i == data.len()).then_some(out)
}

/// 用 refresh token 换 access token：先试上次成功的 client，再依次试 Antigravity 自带的。
/// 返回 `(client_id, access_token, 过期时间)`。
fn refresh(
    refresh_token: &str,
    preferred: Option<&str>,
    http: &HttpFn<'_>,
    clients: &dyn Fn() -> Vec<(String, String)>,
) -> Result<(String, String, i64), String> {
    let mut candidates = clients();
    if candidates.is_empty() {
        return Err("没有在已安装的 Antigravity 中找到 OAuth 参数，无法查询额度".into());
    }
    if let Some(p) = preferred {
        candidates.sort_by_key(|(id, _)| id != p);
    }
    let mut last = String::new();
    for (client_id, secret) in candidates {
        let body = account_store::form_encode(&[
            ("client_id", &client_id),
            ("client_secret", &secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ]);
        let response = http("POST", TOKEN_URL, &[("Content-Type", "application/x-www-form-urlencoded"), ("Accept", "application/json")], Some(body.as_bytes()))?;
        if (200..300).contains(&response.status) {
            let v: Value = serde_json::from_slice(&response.body).map_err(|_| "刷新 token 的响应不是 JSON".to_string())?;
            let access = v.get("access_token").and_then(Value::as_str).ok_or("刷新 token 的响应缺少 access_token")?;
            let expires = Utc::now().timestamp() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
            return Ok((client_id, access.to_string(), expires));
        }
        last = format!("HTTP {} {}", response.status, account_store::error_code(&response.body));
        // 换下一个 client 只在“client 不匹配”时有意义；refresh token 本身失效就直接报错。
        if last.contains("invalid_grant") {
            break;
        }
    }
    Err(format!("刷新 token 失败（{}），请在「添加账号」里重新授权这个账号", last))
}

fn cloud_code_post(path: &str, token: &str, body: &Value, http: &HttpFn<'_>) -> Result<Value, String> {
    let bearer = format!("Bearer {}", token);
    let body = body.to_string();
    let response = http(
        "POST",
        &format!("{}/v1internal:{}", CLOUD_CODE, path),
        &[("Authorization", bearer.as_str()), ("Content-Type", "application/json"), ("Accept", "application/json"), ("User-Agent", USER_AGENT)],
        Some(body.as_bytes()),
    )?;
    match response.status {
        200..=299 => serde_json::from_slice(&response.body).map_err(|_| format!("{} 返回的不是 JSON", path)),
        401 => Err("token 已失效，请在「添加账号」里重新授权这个账号".into()),
        status => Err(format!("{} 返回 HTTP {} {}", path, status, account_store::error_code(&response.body))),
    }
}

/// 返回 `(项目 id, 套餐名)`。
fn load_code_assist(token: &str, http: &HttpFn<'_>) -> Result<(Option<String>, Option<String>), String> {
    let body = json!({
        "metadata": { "ideName": "antigravity", "ideType": "ANTIGRAVITY", "platform": "WINDOWS_AMD64", "pluginType": "GEMINI" },
        "mode": "FULL_ELIGIBILITY_CHECK",
    });
    let v = cloud_code_post("loadCodeAssist", token, &body, http)?;
    let project = match v.get("cloudaicompanionProject") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(obj) => obj.get("id").and_then(Value::as_str).map(str::to_string),
        None => None,
    };
    let tier = |k: &str| v.get(k).and_then(|t| t.get("name").or_else(|| t.get("id"))).and_then(Value::as_str).map(str::to_string);
    Ok((project, tier("paidTier").or_else(|| tier("currentTier"))))
}

fn fetch_models(token: &str, project: Option<String>, http: &HttpFn<'_>) -> Result<Quota, String> {
    let body = match project.filter(|p| !p.is_empty()) {
        Some(p) => json!({ "project": p }),
        None => json!({}),
    };
    let v = cloud_code_post("fetchAvailableModels", token, &body, http)?;
    Ok(parse_models(&v))
}

/// `models` 中用户可选的模型（有显示名且带 `quotaInfo`；没有显示名的是 `chat_*`、`tab_*` 这类内部模型）。
/// `remainingFraction` 缺省表示已用完（proto3 不输出 0）。同名的多个变体合并为一项，保留用得更多的那个。
fn parse_models(v: &Value) -> Quota {
    let mut windows: Vec<QuotaWindow> = Vec::new();
    for (id, m) in v.get("models").and_then(Value::as_object).into_iter().flatten() {
        let (Some(info), Some(label)) = (m.get("quotaInfo"), m.get("displayName").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())) else {
            continue;
        };
        let remaining = info.get("remainingFraction").and_then(Value::as_f64).unwrap_or(0.0);
        let window = QuotaWindow {
            kind: id.clone(),
            label: Some(label.to_string()),
            used_percent: ((1.0 - remaining) * 100.0).clamp(0.0, 100.0),
            window_minutes: None,
            resets_at: info.get("resetTime").and_then(Value::as_str).map(str::to_string),
        };
        match windows.iter_mut().find(|w| w.label == window.label) {
            Some(existing) if existing.used_percent < window.used_percent => *existing = window,
            Some(_) => {}
            None => windows.push(window),
        }
    }
    windows.sort_by(|a, b| a.label.cmp(&b.label));
    Quota { plan_type: None, windows, credits_balance: None }
}

/// 已安装的 Antigravity `language_server.exe` 里的 OAuth client（id 与 secret 的所有组合），只读取一次。
fn oauth_clients() -> &'static [(String, String)] {
    static CLIENTS: OnceLock<Vec<(String, String)>> = OnceLock::new();
    CLIENTS.get_or_init(|| {
        let ids = regex::bytes::Regex::new(r"[0-9]{6,}-[a-z0-9]{20,}\.apps\.googleusercontent\.com").unwrap();
        // Go 程序里的字符串首尾相连、没有分隔符，secret 必须按固定长度（前缀后 28 位）截取。
        let secrets = regex::bytes::Regex::new(r"GOCSPX-[A-Za-z0-9_-]{28}").unwrap();
        let programs = dirs::data_local_dir().unwrap_or_default().join("Programs");
        let mut out = Vec::new();
        for dir in ["antigravity", "Antigravity", "Antigravity IDE"] {
            let bin = programs.join(dir).join("resources").join("bin");
            let Ok(entries) = std::fs::read_dir(&bin) else { continue };
            for path in entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("language_server"))) {
                let Ok(bytes) = std::fs::read(&path) else { continue };
                let found_ids: Vec<String> = ids.find_iter(&bytes).map(|m| String::from_utf8_lossy(m.as_bytes()).to_string()).collect();
                let found_secrets: Vec<String> = secrets.find_iter(&bytes).map(|m| String::from_utf8_lossy(m.as_bytes()).to_string()).collect();
                for id in &found_ids {
                    for secret in &found_secrets {
                        if !out.contains(&(id.clone(), secret.clone())) {
                            out.push((id.clone(), secret.clone()));
                        }
                    }
                }
            }
            if !out.is_empty() {
                break;
            }
        }
        out
    })
}

fn view(a: &StoredAccount, is_current: bool) -> AccountView {
    AccountView {
        id: a.id.clone(),
        email: Some(a.email.clone()),
        name: a.name.clone(),
        plan_type: a.quota.quota.as_ref().and_then(|q| q.plan_type.clone()),
        is_current,
        access_expires_at: token_info(&a.items)
            .and_then(|(_, _, exp)| exp)
            .and_then(|t| Utc.timestamp_opt(t, 0).single())
            .map(|t| t.to_rfc3339()),
        added_at: a.added_at.clone(),
        last_used_at: a.last_used_at.clone(),
        has_quota: true,
        ..Default::default()
    }
    .with_quota(&a.quota)
}

// ---------------------------------------------------------------------------
// 不经过 Antigravity 添加账号：在浏览器里用 Google 授权，或导入 refresh token。
//
// 这样得到的账号没有 Antigravity 写下的快照，切换时按 Antigravity 的格式合成登录项
// （oauthToken、userStatus、authStatus）；Antigravity 启动后会自己补全其他数据，下次切走时
// 再保存的就是完整快照。
// ---------------------------------------------------------------------------

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const CALLBACK_PATH: &str = "/oauth-callback";
/// Antigravity 自己登录时申请的权限。
const SCOPES: &[&str] = &[
    "openid",
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
    "https://www.googleapis.com/auth/aicode",
];
const USER_STATUS_KEY: &str = "antigravityUnifiedStateSync.userStatus";
const ENTERPRISE_KEY: &str = "antigravityUnifiedStateSync.enterprisePreferences";
/// 等待浏览器授权的最长时间。
const LOGIN_TIMEOUT_SECS: u64 = 600;

/// 一次导入 refresh token 的结果。
#[derive(Debug, Default, Serialize)]
pub struct TokenImport {
    pub ids: Vec<String>,
    /// 每项为“账号或 token 前几位：原因”。
    pub failures: Vec<String>,
}

impl AntigravityAccountManager {
    /// 在浏览器里用 Google 账号授权并保存为账号，不影响 Antigravity 当前的登录。返回账号 id。
    /// `on_url` 收到授权链接（浏览器没自动打开时可手动打开）；`cancelled` 为真时放弃等待。
    pub fn login_in_browser(&self, on_url: &dyn Fn(&str), cancelled: &dyn Fn() -> bool) -> Result<String, String> {
        self.login_in_browser_with(on_url, cancelled, &win_http::request, &|| oauth_clients().to_vec(), &open_in_browser)
    }

    fn login_in_browser_with(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        http: &HttpFn<'_>,
        clients: &dyn Fn() -> Vec<(String, String)>,
        open: &dyn Fn(&str),
    ) -> Result<String, String> {
        let (client_id, secret) = self.login_client(http, clients)?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("无法在本机打开授权回调端口：{}", e))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect = format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH);
        let state = uuid::Uuid::new_v4().simple().to_string();
        // PKCE：即使授权码被截获，没有 verifier 也换不到 token。
        let verifier = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let challenge = base64_url(&Sha256::digest(verifier.as_bytes()));
        let url = format!(
            "{}?{}",
            AUTH_URL,
            account_store::form_encode(&[
                ("client_id", &client_id),
                ("redirect_uri", &redirect),
                ("response_type", "code"),
                ("scope", &SCOPES.join(" ")),
                ("access_type", "offline"),
                ("prompt", "consent select_account"),
                ("include_granted_scopes", "true"),
                ("state", &state),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
            ])
        );
        on_url(&url);
        open(&url);
        let code = wait_for_code(&listener, &state, cancelled)?;
        let body = account_store::form_encode(&[
            ("code", &code),
            ("client_id", &client_id),
            ("client_secret", &secret),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ]);
        let response = http("POST", TOKEN_URL, &[("Content-Type", "application/x-www-form-urlencoded"), ("Accept", "application/json")], Some(body.as_bytes()))?;
        if !(200..300).contains(&response.status) {
            return Err(format!("用授权码换 token 失败（HTTP {} {}），请重试", response.status, account_store::error_code(&response.body)));
        }
        let v: Value = serde_json::from_slice(&response.body).map_err(|_| "换取 token 的响应不是 JSON".to_string())?;
        let access = v.get("access_token").and_then(Value::as_str).ok_or("换取 token 的响应缺少 access_token")?;
        let refresh_token = v
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or("Google 没有返回 refresh token，请重新授权并勾选全部权限")?;
        let expires_at = Utc::now().timestamp() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        self.add_tokens(&client_id, access, refresh_token, expires_at, None, http)
    }

    /// 导入 refresh token：每行一个，或账号 JSON（数组或单个对象，含 `refresh_token` 和可选的 `email`）。
    pub fn import_tokens(&self, text: &str) -> Result<TokenImport, String> {
        self.import_tokens_with(text, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn import_tokens_with(&self, text: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<TokenImport, String> {
        let entries = parse_token_input(text);
        if entries.is_empty() {
            return Err("没有找到 refresh token：请粘贴以 1// 开头的 token，或含 refresh_token 的账号 JSON".into());
        }
        let mut result = TokenImport::default();
        for (refresh_token, email) in entries {
            let label = email.clone().unwrap_or_else(|| mask_token(&refresh_token));
            let outcome = refresh(&refresh_token, None, http, clients)
                .and_then(|(client_id, access, expires_at)| self.add_tokens(&client_id, &access, &refresh_token, expires_at, email.as_deref(), http));
            match outcome {
                Ok(id) => result.ids.push(id),
                Err(e) => result.failures.push(format!("{}：{}", label, e)),
            }
        }
        Ok(result)
    }

    /// 浏览器授权用的 OAuth client：已有账号刷新成功过的那个，否则是能刷新 Antigravity 当前登录的那个，
    /// 再否则是第一组被 Google 认可的 id 与 secret。
    fn login_client(&self, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<(String, String), String> {
        let candidates = clients();
        if candidates.is_empty() {
            return Err("没有在已安装的 Antigravity 中找到 OAuth 参数，请先安装 Antigravity".into());
        }
        let used = self.load().ok().and_then(|s| s.accounts.iter().find_map(|a| a.client_id.clone()));
        let live = self.read_live().ok().flatten().and_then(|items| token_info(&items)).map(|(_, refresh_token, _)| refresh_token);
        let preferred = used.or_else(|| live.and_then(|t| refresh(&t, None, http, clients).ok().map(|(id, _, _)| id)));
        let mut ordered = candidates.clone();
        if let Some(p) = &preferred {
            ordered.sort_by_key(|(id, _)| id != p);
        }
        // id 与 secret 是分别从程序里找到的，只有配对的组合能通过 Google 的 client 校验。
        for (id, secret) in &ordered {
            if client_pair_valid(id, secret, http) {
                return Ok((id.clone(), secret.clone()));
            }
        }
        Err("Antigravity 里找到的 OAuth 参数都没有通过 Google 校验，请更新 Antigravity 后重试".into())
    }

    /// 用一组 token 新建账号，或更新同邮箱的账号；之后切换到它时按这组 token 合成登录项。
    fn add_tokens(&self, client_id: &str, access: &str, refresh_token: &str, expires_at: i64, email_hint: Option<&str>, http: &HttpFn<'_>) -> Result<String, String> {
        let bearer = format!("Bearer {}", access);
        let profile = http("GET", USERINFO_URL, &[("Authorization", bearer.as_str()), ("Accept", "application/json")], None)
            .ok()
            .filter(|r| (200..300).contains(&r.status))
            .and_then(|r| serde_json::from_slice::<Value>(&r.body).ok());
        let email = profile
            .as_ref()
            .and_then(|p| p.get("email").and_then(Value::as_str))
            .or(email_hint)
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .ok_or("无法确认这个 token 属于哪个 Google 账号，请在账号 JSON 里写上 email")?
            .to_string();
        let name = profile.as_ref().and_then(|p| p.get("name").and_then(Value::as_str)).map(str::to_string);
        let mut store = self.load()?;
        let id = match store.accounts.iter_mut().find(|a| a.email.eq_ignore_ascii_case(&email)) {
            Some(existing) => {
                existing.items = synth_items(&email, name.as_deref().or(existing.name.as_deref()), access, refresh_token, expires_at, existing.project_id.as_deref());
                existing.synthetic = true;
                if name.is_some() {
                    existing.name = name;
                }
                existing.client_id = Some(client_id.to_string());
                existing.access_token = Some(access.to_string());
                existing.access_expires_at = Some(expires_at);
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    items: synth_items(&email, name.as_deref(), access, refresh_token, expires_at, None),
                    email,
                    name,
                    synthetic: true,
                    client_id: Some(client_id.to_string()),
                    access_token: Some(access.to_string()),
                    access_expires_at: Some(expires_at),
                    project_id: None,
                    added_at: account_store::now(),
                    last_used_at: None,
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }
}

/// 等浏览器回到本机端口，取出授权码；其他请求（如 favicon）回 404 后继续等。
fn wait_for_code(listener: &TcpListener, state: &str, cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if cancelled() {
            return Err("已取消登录".into());
        }
        if started.elapsed() > Duration::from_secs(LOGIN_TIMEOUT_SECS) {
            return Err("等待浏览器授权超时，请重试".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let outcome = parse_callback(request.lines().next().unwrap_or(""), state);
                let (status, page) = match &outcome {
                    None => ("404 Not Found", String::new()),
                    Some(Ok(_)) => ("200 OK", callback_page("授权完成", "账号已添加到 Context Vault，可以关闭这个页面了。")),
                    Some(Err(e)) => ("400 Bad Request", callback_page("授权没有完成", e)),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    page.len(),
                    page
                );
                if let Some(result) = outcome {
                    return result;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("授权回调出错：{}", e)),
        }
    }
}

/// 解析回调请求行 `GET /oauth-callback?code=…&state=… HTTP/1.1`；不是回调地址时为空。
fn parse_callback(request_line: &str, state: &str) -> Option<Result<String, String>> {
    let target = request_line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return None;
    }
    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (percent_decode(k), percent_decode(v)))
        .collect();
    if let Some(error) = params.get("error") {
        return Some(Err(match error.as_str() {
            "access_denied" => "你在浏览器里取消了授权".to_string(),
            other => format!("Google 拒绝了授权：{}", other),
        }));
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Some(Err("授权回调和本次登录对不上，请重新点「打开浏览器登录」".into()));
    }
    Some(params.get("code").filter(|c| !c.is_empty()).cloned().ok_or_else(|| "授权回调里没有授权码，请重试".to_string()))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(high), Some(low)) => {
                        out.push((high * 16 + low) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn callback_page(title: &str, message: &str) -> String {
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

/// 用一个必然无效的授权码试探：id 与 secret 配对时 Google 回 invalid_grant，不配对时回 invalid_client。
fn client_pair_valid(client_id: &str, secret: &str, http: &HttpFn<'_>) -> bool {
    let body = account_store::form_encode(&[
        ("client_id", client_id),
        ("client_secret", secret),
        ("code", "probe"),
        ("grant_type", "authorization_code"),
        ("redirect_uri", "http://127.0.0.1"),
    ]);
    http("POST", TOKEN_URL, &[("Content-Type", "application/x-www-form-urlencoded"), ("Accept", "application/json")], Some(body.as_bytes()))
        .map(|r| account_store::error_code(&r.body).contains("invalid_grant"))
        .unwrap_or(false)
}

/// 粘贴的内容里的 `(refresh_token, email)`：账号 JSON 优先，否则按 `1//…` 逐个取出。重复的只算一次。
fn parse_token_input(text: &str) -> Vec<(String, Option<String>)> {
    fn push(out: &mut Vec<(String, Option<String>)>, token: &str, email: Option<&str>) {
        let token = token.trim();
        if !token.is_empty() && !out.iter().any(|(t, _)| t == token) {
            out.push((token.to_string(), email.map(str::trim).filter(|e| !e.is_empty()).map(str::to_string)));
        }
    }
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(text.trim()) {
        let items: Vec<&Value> = match &value {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => match o.get("accounts") {
                Some(Value::Array(a)) => a.iter().collect(),
                _ => vec![&value],
            },
            _ => Vec::new(),
        };
        for item in items {
            let token = ["refresh_token", "refreshToken"]
                .iter()
                .find_map(|k| item.get(*k).or_else(|| item.get("token").and_then(|t| t.get(*k))))
                .and_then(Value::as_str);
            if let Some(token) = token {
                push(&mut out, token, item.get("email").and_then(Value::as_str));
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let pattern = regex::Regex::new(r"1//[0-9A-Za-z_\-]{20,}").unwrap();
    for m in pattern.find_iter(text) {
        push(&mut out, m.as_str(), None);
    }
    out
}

/// 报错时只露出 token 的开头，避免把整段 token 显示在界面上。
fn mask_token(token: &str) -> String {
    format!("{}…", token.chars().take(10).collect::<String>())
}

/// 按 Antigravity 的格式合成一个账号的登录项。
fn synth_items(email: &str, name: Option<&str>, access: &str, refresh_token: &str, expires_at: i64, project: Option<&str>) -> Map<String, Value> {
    let item = |text: String| serde_json::to_value(Item::Text(text)).unwrap_or(Value::Null);
    let mut info = Vec::new();
    pb_bytes(1, access.as_bytes(), &mut info);
    pb_bytes(2, b"Bearer", &mut info);
    pb_bytes(3, refresh_token.as_bytes(), &mut info);
    let mut expiry = Vec::new();
    pb_varint_field(1, expires_at.max(0) as u64, &mut expiry);
    pb_bytes(4, &expiry, &mut info);
    let mut user_status = Vec::new();
    pb_bytes(3, email.as_bytes(), &mut user_status);
    pb_bytes(7, email.as_bytes(), &mut user_status);

    let mut items = Map::new();
    items.insert(TOKEN_KEY.into(), item(account_store::base64_encode(&topic_entry(TOKEN_SENTINEL, &info))));
    items.insert(USER_STATUS_KEY.into(), item(account_store::base64_encode(&topic_entry("userStatusSentinelKey", &user_status))));
    let auth = json!({
        "apiKey": access,
        "email": email,
        "name": name.unwrap_or(email),
        "userStatusProtoBinaryBase64": account_store::base64_encode(&user_status),
    });
    items.insert(AUTH_KEY.into(), item(auth.to_string()));
    if let Some(project) = project.map(str::trim).filter(|p| !p.is_empty() && *p != "aicode-consumers") {
        let mut value = Vec::new();
        pb_bytes(3, project.as_bytes(), &mut value);
        items.insert(ENTERPRISE_KEY.into(), item(account_store::base64_encode(&topic_entry("enterpriseGcpProjectId", &value))));
    }
    items
}

/// unified state 的一项：`topic{ entry{ 1: key, 2: row{ 1: base64(payload) } } }`。
fn topic_entry(key: &str, payload: &[u8]) -> Vec<u8> {
    let mut row = Vec::new();
    pb_bytes(1, account_store::base64_encode(payload).as_bytes(), &mut row);
    let mut entry = Vec::new();
    pb_bytes(1, key.as_bytes(), &mut entry);
    pb_bytes(2, &row, &mut entry);
    let mut topic = Vec::new();
    pb_bytes(1, &entry, &mut topic);
    topic
}

fn pb_varint(mut n: u64, out: &mut Vec<u8>) {
    while n >= 0x80 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn pb_varint_field(field: u32, value: u64, out: &mut Vec<u8>) {
    pb_varint((field << 3) as u64, out);
    pb_varint(value, out);
}

fn pb_bytes(field: u32, data: &[u8], out: &mut Vec<u8>) {
    pb_varint(((field << 3) | 2) as u64, out);
    pb_varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

fn base64_url(data: &[u8]) -> String {
    account_store::base64_encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// 用默认浏览器打开链接。
fn open_in_browser(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).creation_flags(CREATE_NO_WINDOW).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;
    use std::io::{Read, Write};

    fn varint(mut n: u64, out: &mut Vec<u8>) {
        while n >= 0x80 {
            out.push((n as u8) | 0x80);
            n >>= 7;
        }
        out.push(n as u8);
    }
    fn bytes_field(field: u32, data: &[u8], out: &mut Vec<u8>) {
        varint(((field << 3) | 2) as u64, out);
        varint(data.len() as u64, out);
        out.extend_from_slice(data);
    }

    /// 与 Antigravity 相同结构的 oauthToken：topic{ entry{ key, value{ base64(OAuthTokenInfo) } } }。
    fn oauth_token(access: &str, refresh: &str, expiry: u64) -> String {
        let mut info = Vec::new();
        bytes_field(1, access.as_bytes(), &mut info);
        bytes_field(2, b"Bearer", &mut info);
        bytes_field(3, refresh.as_bytes(), &mut info);
        let mut ts = Vec::new();
        varint(1 << 3, &mut ts);
        varint(expiry, &mut ts);
        bytes_field(4, &ts, &mut info);
        let mut wrapper = Vec::new();
        bytes_field(1, account_store::base64_encode(&info).as_bytes(), &mut wrapper);
        let mut entry = Vec::new();
        bytes_field(1, TOKEN_SENTINEL.as_bytes(), &mut entry);
        bytes_field(2, &wrapper, &mut entry);
        let mut topic = Vec::new();
        bytes_field(1, &entry, &mut topic);
        account_store::base64_encode(&topic)
    }

    struct Sandbox {
        root: PathBuf,
        m: AntigravityAccountManager,
    }
    impl Sandbox {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cv-ag-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let db = root.join("state.vscdb");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB); INSERT INTO ItemTable VALUES ('theme', 'dark');").unwrap();
            let m = AntigravityAccountManager::new(&db, &root.join("data"), &root.join("backups"));
            Self { root, m }
        }
        fn login(&self, email: &str, refresh: &str, credits: Option<&str>) {
            let conn = Connection::open(self.root.join("state.vscdb")).unwrap();
            let auth = json!({ "name": email.split('@').next(), "email": email, "apiKey": "ya29.x" }).to_string();
            conn.execute("INSERT OR REPLACE INTO ItemTable VALUES (?1, ?2)", rusqlite::params![AUTH_KEY, auth]).unwrap();
            conn.execute("INSERT OR REPLACE INTO ItemTable VALUES (?1, ?2)", rusqlite::params![TOKEN_KEY, oauth_token("ya29.old", refresh, 1_777_214_128)]).unwrap();
            match credits {
                Some(c) => conn.execute("INSERT OR REPLACE INTO ItemTable VALUES ('antigravityUnifiedStateSync.modelCredits', ?1)", [c]).unwrap(),
                None => conn.execute("DELETE FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.modelCredits'", []).unwrap(),
            };
        }
        fn value(&self, key: &str) -> Option<String> {
            let conn = Connection::open(self.root.join("state.vscdb")).unwrap();
            conn.query_row("SELECT value FROM ItemTable WHERE key = ?", [key], |r| r.get(0)).optional().unwrap()
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn parses_the_oauth_token_protobuf() {
        let mut items = Map::new();
        items.insert(TOKEN_KEY.into(), serde_json::to_value(Item::Text(oauth_token("ya29.a", "1//r", 1_777_214_128))).unwrap());
        assert_eq!(token_info(&items), Some(("ya29.a".into(), "1//r".into(), Some(1_777_214_128))));
        items.insert(TOKEN_KEY.into(), serde_json::to_value(Item::Text("not-protobuf".into())).unwrap());
        assert_eq!(token_info(&items), None);
    }

    #[test]
    fn saves_and_switches_only_account_keys() {
        let s = Sandbox::new();
        s.login("a@gmail.com", "1//a", Some("credits-a"));
        let id_a = s.m.save_current().unwrap();
        s.login("b@gmail.com", "1//b", None);
        let id_b = s.m.save_current().unwrap();
        assert_eq!(s.m.save_current().unwrap(), id_b);
        s.m.switch(&id_a).unwrap();
        assert!(s.value(AUTH_KEY).unwrap().contains("a@gmail.com"));
        assert_eq!(s.value("antigravityUnifiedStateSync.modelCredits").as_deref(), Some("credits-a"));
        assert_eq!(s.value("theme").as_deref(), Some("dark"));
        assert_eq!(s.value("antigravityOnboarding").as_deref(), Some("true"));
        let state = s.m.state().unwrap();
        assert!(state.accounts.iter().find(|a| a.id == id_a).unwrap().is_current);
        // b 没有 modelCredits：切回 b 时删掉 a 的这一项。
        s.m.switch(&id_b).unwrap();
        assert_eq!(s.value("antigravityUnifiedStateSync.modelCredits"), None);
        assert!(s.value(AUTH_KEY).unwrap().contains("b@gmail.com"));
        assert_eq!(fs::read_dir(s.root.join("backups/accounts/antigravity")).unwrap().count(), 2);
        s.m.delete(&id_a).unwrap();
        assert_eq!(s.m.state().unwrap().accounts.len(), 1);
    }

    #[test]
    fn quota_tries_clients_then_reads_models() {
        let s = Sandbox::new();
        s.login("a@gmail.com", "1//a", None);
        let id = s.m.save_current().unwrap();
        let calls = RefCell::new(Vec::<String>::new());
        let http = |method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>| {
            let body = String::from_utf8_lossy(body.unwrap_or_default()).to_string();
            calls.borrow_mut().push(format!("{method} {url}"));
            if url == TOKEN_URL {
                assert!(body.contains("refresh_token=1%2F%2Fa"));
                return Ok(if body.contains("client_id=good") {
                    win_http::HttpResponse { status: 200, body: br#"{"access_token":"ya29.new","expires_in":3600}"#.to_vec() }
                } else {
                    win_http::HttpResponse { status: 401, body: br#"{"error":"unauthorized_client"}"#.to_vec() }
                });
            }
            assert!(headers.contains(&("Authorization", "Bearer ya29.new")));
            Ok(win_http::HttpResponse {
                status: 200,
                body: if url.ends_with("loadCodeAssist") {
                    br#"{"cloudaicompanionProject":"proj-1","currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#.to_vec()
                } else {
                    assert_eq!(body, r#"{"project":"proj-1"}"#);
                    br#"{"models":{"gemini-3-pro":{"displayName":"Gemini 3 Pro","quotaInfo":{"remainingFraction":0.8,"resetTime":"2026-10-24T00:00:00Z"}},"claude-sonnet":{"displayName":"Claude Sonnet","quotaInfo":{"resetTime":"2026-10-24T05:00:00Z"}},"internal":{"displayName":"Tab"},"chat_1":{"quotaInfo":{"remainingFraction":1}},"gemini-3-pro-low":{"displayName":"Gemini 3 Pro","quotaInfo":{"remainingFraction":0.95}}}}"#.to_vec()
                },
            })
        };
        let clients = || vec![("bad".to_string(), "s".to_string()), ("good".to_string(), "s".to_string())];
        s.m.refresh_quota_with(&id, &http, &clients).unwrap();
        let quota = s.m.state().unwrap().accounts[0].quota.clone().unwrap();
        assert_eq!(quota.plan_type.as_deref(), Some("Google AI Pro"));
        let models: Vec<_> = quota.windows.iter().map(|w| (w.label.clone().unwrap(), w.used_percent.round() as i64)).collect();
        assert_eq!(models, vec![("Claude Sonnet".to_string(), 100), ("Gemini 3 Pro".to_string(), 20)]);
        assert_eq!(calls.borrow().len(), 4);
        // access token 仍有效时不再刷新，直接查询。
        calls.borrow_mut().clear();
        s.m.refresh_quota_with(&id, &http, &clients).unwrap();
        assert!(calls.borrow().iter().all(|c| !c.contains("oauth2")));
    }

    /// 假的 Google：`good` 这组 client 被认可，refresh token `1//good…` 能换到 access token，userinfo 返回 `who`。
    fn fake_google<'a>(who: &'a str, calls: &'a RefCell<Vec<String>>) -> impl Fn(&str, &str, &[(&str, &str)], Option<&[u8]>) -> Result<win_http::HttpResponse, String> + 'a {
        move |method: &str, url: &str, _headers: &[(&str, &str)], body: Option<&[u8]>| {
            let body = String::from_utf8_lossy(body.unwrap_or_default()).to_string();
            calls.borrow_mut().push(format!("{method} {url} {body}"));
            let reply = |status: u16, text: &str| Ok(win_http::HttpResponse { status, body: text.as_bytes().to_vec() });
            if url == USERINFO_URL {
                return reply(200, &format!(r#"{{"email":"{}","name":"Who"}}"#, who));
            }
            assert_eq!(url, TOKEN_URL);
            if !body.contains("client_id=good") {
                return reply(401, r#"{"error":"invalid_client"}"#);
            }
            if body.contains("code=probe") {
                return reply(400, r#"{"error":"invalid_grant"}"#);
            }
            if body.contains("grant_type=authorization_code") {
                assert!(body.contains("code=the-code") && body.contains("code_verifier="));
                return reply(200, r#"{"access_token":"ya29.fresh","refresh_token":"1//fresh-refresh-token-0123456789","expires_in":3600}"#);
            }
            if body.contains("refresh_token=1%2F%2Fgood") || body.contains("refresh_token=1%2F%2Ffresh") {
                return reply(200, r#"{"access_token":"ya29.renewed","expires_in":3600}"#);
            }
            reply(400, r#"{"error":"invalid_grant"}"#)
        }
    }

    fn clients() -> Vec<(String, String)> {
        vec![("bad".to_string(), "s1".to_string()), ("good".to_string(), "s2".to_string())]
    }

    #[test]
    fn callback_requests_are_checked_for_path_state_and_errors() {
        assert_eq!(parse_callback("GET /oauth-callback?state=s1&code=4%2F0Ab HTTP/1.1", "s1"), Some(Ok("4/0Ab".into())));
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s1"), None);
        assert!(parse_callback("GET /oauth-callback?state=other&code=x HTTP/1.1", "s1").unwrap().is_err());
        assert_eq!(parse_callback("GET /oauth-callback?error=access_denied&state=s1 HTTP/1.1", "s1"), Some(Err("你在浏览器里取消了授权".into())));
    }

    #[test]
    fn pasted_tokens_are_read_from_lines_or_account_json() {
        let lines = "1//aaaaaaaaaaaaaaaaaaaaaaaa\n  1//bbbbbbbbbbbbbbbbbbbbbbbb\n1//aaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(parse_token_input(lines).len(), 2);
        let json = r#"[{"email":"a@gmail.com","refresh_token":"1//x"},{"token":{"refresh_token":"1//y"}},{"email":"none"}]"#;
        assert_eq!(parse_token_input(json), vec![("1//x".to_string(), Some("a@gmail.com".to_string())), ("1//y".to_string(), None)]);
        assert_eq!(parse_token_input(r#"{"accounts":[{"refreshToken":"1//z","email":"z@gmail.com"}]}"#)[0].1.as_deref(), Some("z@gmail.com"));
        assert!(parse_token_input("not a token").is_empty());
    }

    #[test]
    fn synthesized_items_read_back_like_antigravity_login() {
        let items = synth_items("a@gmail.com", Some("A"), "ya29.a", "1//r", 1_777_214_128, Some("proj-1"));
        assert_eq!(token_info(&items), Some(("ya29.a".into(), "1//r".into(), Some(1_777_214_128))));
        assert_eq!(identity(items.get(AUTH_KEY)), Some(("a@gmail.com".into(), Some("A".into()))));
        assert!(items.contains_key(USER_STATUS_KEY) && items.contains_key(ENTERPRISE_KEY));
        assert!(!synth_items("a@gmail.com", None, "t", "r", 0, Some("aicode-consumers")).contains_key(ENTERPRISE_KEY));
    }

    #[test]
    fn imported_tokens_become_accounts_that_switch_without_antigravity_snapshots() {
        let s = Sandbox::new();
        let calls = RefCell::new(Vec::new());
        let http = fake_google("new@gmail.com", &calls);
        let result = s.m.import_tokens_with("1//good-refresh-token-0123456789\n1//dead-refresh-token-0123456789", &http, &clients).unwrap();
        assert_eq!(result.ids.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0].starts_with("1//dead-re…："), "{:?}", result.failures);
        let id = result.ids[0].clone();
        assert_eq!(s.m.state().unwrap().accounts[0].email.as_deref(), Some("new@gmail.com"));
        assert!(s.m.import_tokens_with("nothing here", &http, &clients).is_err());

        // 切换时换一个新的 access token，按 Antigravity 的格式写入。
        s.m.switch_with(&id, &http, &clients).unwrap();
        assert!(s.value(AUTH_KEY).unwrap().contains("new@gmail.com"));
        let mut live = Map::new();
        live.insert(TOKEN_KEY.into(), serde_json::to_value(Item::Text(s.value(TOKEN_KEY).unwrap())).unwrap());
        assert_eq!(token_info(&live).map(|(a, r, _)| (a, r)), Some(("ya29.renewed".into(), "1//good-refresh-token-0123456789".into())));
        assert!(s.value(USER_STATUS_KEY).is_some());
        assert!(s.m.state().unwrap().accounts[0].is_current);

        // Antigravity 登录后写下完整快照，保存时不再是合成的。
        s.login("new@gmail.com", "1//good-refresh-token-0123456789", Some("credits"));
        s.m.save_current().unwrap();
        assert!(!s.m.load().unwrap().accounts[0].synthetic);
    }

    #[test]
    fn browser_login_exchanges_the_callback_code_and_saves_the_account() {
        let s = Sandbox::new();
        let calls = RefCell::new(Vec::new());
        let http = fake_google("browser@gmail.com", &calls);
        let shown = RefCell::new(String::new());
        // 假的浏览器：从授权链接里取出回调地址和 state，像 Google 一样跳回本机。
        let open = |url: &str| {
            let query = url.split_once('?').unwrap().1;
            let param = |k: &str| query.split('&').find_map(|p| p.strip_prefix(&format!("{k}="))).map(percent_decode).unwrap();
            assert!(url.contains("code_challenge_method=S256") && url.contains("access_type=offline"));
            assert_eq!(param("client_id"), "good");
            let (redirect, state) = (param("redirect_uri"), param("state"));
            std::thread::spawn(move || {
                let address = redirect.trim_start_matches("http://").split('/').next().unwrap().to_string();
                for path in ["/favicon.ico".to_string(), format!("{}?code=the-code&state={}", CALLBACK_PATH, state)] {
                    let mut stream = std::net::TcpStream::connect(&address).unwrap();
                    write!(stream, "GET {} HTTP/1.1\r\nHost: {}\r\n\r\n", path, address).unwrap();
                    let mut reply = String::new();
                    let _ = stream.read_to_string(&mut reply);
                }
            });
        };
        let id = s.m.login_in_browser_with(&|url| *shown.borrow_mut() = url.to_string(), &|| false, &http, &clients, &open).unwrap();
        assert!(shown.borrow().starts_with(AUTH_URL));
        let account = s.m.load().unwrap().accounts.into_iter().find(|a| a.id == id).unwrap();
        assert_eq!(account.email, "browser@gmail.com");
        assert!(account.synthetic);
        assert_eq!(token_info(&account.items).map(|(_, r, _)| r).as_deref(), Some("1//fresh-refresh-token-0123456789"));
        assert_eq!(account.client_id.as_deref(), Some("good"));

        // 取消时立刻停止等待。
        let err = s.m.login_in_browser_with(&|_| {}, &|| true, &http, &clients, &|_| {}).unwrap_err();
        assert_eq!(err, "已取消登录");
    }
}

#[cfg(test)]
mod local_install {
    /// 只读检查本机安装：cargo test local_install -- --ignored --nocapture
    #[test]
    #[ignore]
    fn reads_the_real_state_db_and_finds_oauth_clients() {
        let db = super::default_state_db();
        let tmp = std::env::temp_dir().join(format!("cv-ag-check-{}", uuid::Uuid::new_v4()));
        let m = super::AntigravityAccountManager::new(&db, &tmp, &tmp);
        let items = m.read_live().unwrap().expect("没有 Antigravity 登录");
        let (access, refresh, exp) = super::token_info(&items).expect("解析 oauthToken 失败");
        println!("live token: access {} chars, refresh {} chars, expiry {:?}", access.len(), refresh.len(), exp);
        println!("oauth clients found: {}", super::oauth_clients().len());
        println!("claude client id found: {}", super::super::claude_accounts::find_claude_client_id().is_some());
        assert!(!super::oauth_clients().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
