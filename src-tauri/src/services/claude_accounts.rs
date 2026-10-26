//! Claude Code 账号：官方订阅登录（OAuth）与中转供应商（BASE_URL + TOKEN）。
//!
//! - 官方登录 = `~/.claude/.credentials.json` 的 `claudeAiOauth` + `~/.claude.json` 的 `oauthAccount`。
//! - 中转供应商 = `~/.claude/settings.json` 里 `env` 的 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN` 等。
//!   `env` 里有这些值时 Claude Code 用中转，官方登录不生效；切到官方账号时会移除它们。
//!
//! 账号保存在本程序数据目录的 `accounts/claude.json`，与 Codex、Antigravity 互不相干。
//! 官方额度来自 `api.anthropic.com/api/oauth/usage`。当前登录的账号不由这里刷新 token
//! （Anthropic 的 refresh token 用一次就作废），其他账号过期时用它的 refresh token 换新；
//! 刷新用的 client id 在运行时从已安装的 Claude Code 中读取。

use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::account_store::{self, AccountView, AccountsState, CurrentLogin, Quota, QuotaState, QuotaWindow};
use super::win_http::{self, HttpFn};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const STORE_FILE: &str = "claude.json";
const EXPIRY_MARGIN_MS: i64 = 120_000;
/// `settings.json` 的 `env` 中属于中转供应商的键；切换供应商时整组替换。
pub const PROVIDER_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Oauth,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    id: String,
    kind: Kind,
    /// 官方登录：`claudeAiOauth` 对象。
    #[serde(default)]
    credentials: Option<Value>,
    /// 官方登录：`oauthAccount` 对象。
    #[serde(default)]
    oauth_account: Option<Value>,
    /// 中转供应商：显示名。
    #[serde(default)]
    name: Option<String>,
    /// 中转供应商：`PROVIDER_KEYS` 中的键值。
    #[serde(default)]
    env: Map<String, Value>,
    added_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(flatten)]
    quota: QuotaState,
}

impl StoredAccount {
    fn key(&self) -> Option<String> {
        match self.kind {
            Kind::Oauth => oauth_key(self.oauth_account.as_ref()?),
            Kind::Provider => provider_key(&self.env),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: Vec<StoredAccount>,
}

/// 新增或编辑中转供应商。`token` 为空表示编辑时保留原值。
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub token: Option<String>,
    pub model: Option<String>,
}

pub struct ClaudeAccountManager {
    claude_home: PathBuf,
    claude_json: PathBuf,
    store_path: PathBuf,
    backups: PathBuf,
}

fn oauth_key(oauth_account: &Value) -> Option<String> {
    let uuid = oauth_account.get("accountUuid").and_then(Value::as_str)?;
    let org = oauth_account.get("organizationUuid").and_then(Value::as_str).unwrap_or("");
    Some(format!("oauth|{}|{}", uuid, org))
}

fn provider_key(env: &Map<String, Value>) -> Option<String> {
    let url = env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str).unwrap_or("").trim().trim_end_matches('/');
    let token = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if url.is_empty() && token.is_empty() {
        return None;
    }
    let digest = Sha256::digest(token.as_bytes());
    Some(format!("provider|{}|{:x}", url.to_lowercase(), digest))
}

fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

impl ClaudeAccountManager {
    pub fn new(claude_home: &Path, claude_json: &Path, data_dir: &Path, backups: &Path) -> Self {
        Self {
            claude_home: claude_home.to_path_buf(),
            claude_json: claude_json.to_path_buf(),
            store_path: data_dir.join("accounts").join(STORE_FILE),
            backups: backups.join("accounts").join("claude"),
        }
    }

    fn credentials_path(&self) -> PathBuf {
        self.claude_home.join(".credentials.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.claude_home.join("settings.json")
    }

    fn load(&self) -> Result<Store, String> {
        account_store::load_json(&self.store_path)
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        account_store::save_json(&self.store_path, &json!({ "version": 1, "accounts": store.accounts }))
    }

    /// 当前的官方登录：`(claudeAiOauth, oauthAccount)`。
    fn live_oauth(&self) -> Option<(Value, Option<Value>)> {
        let creds: Value = account_store::load_json::<Value>(&self.credentials_path()).ok()?;
        let oauth = creds.get("claudeAiOauth").filter(|v| v.get("accessToken").is_some())?.clone();
        let account = account_store::load_json::<Value>(&self.claude_json).ok().and_then(|v| v.get("oauthAccount").cloned());
        Some((oauth, account))
    }

    /// `settings.json` 中生效的中转供应商（有地址或 token 时）。
    fn live_provider(&self) -> Option<Map<String, Value>> {
        let settings: Value = account_store::load_json::<Value>(&self.settings_path()).ok()?;
        let env = settings.get("env")?.as_object()?;
        let picked: Map<String, Value> = PROVIDER_KEYS
            .iter()
            .filter_map(|k| env.get(*k).filter(|v| v.as_str().is_some_and(|s| !s.is_empty())).map(|v| (k.to_string(), v.clone())))
            .collect();
        ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
            .iter()
            .any(|k| picked.contains_key(*k))
            .then_some(picked)
    }

    pub fn state(&self) -> Result<AccountsState, String> {
        let store = self.load()?;
        let provider = self.live_provider();
        let oauth = self.live_oauth();
        let provider_key = provider.as_ref().and_then(provider_key);
        let oauth_key = oauth.as_ref().and_then(|(_, a)| a.as_ref()).and_then(oauth_key);
        let find = |key: &Option<String>| key.as_ref().and_then(|k| store.accounts.iter().find(|a| a.key().as_ref() == Some(k)));

        let current = if let Some(env) = &provider {
            let saved = find(&provider_key);
            Some(CurrentLogin {
                kind: Some("provider".into()),
                name: saved
                    .and_then(|a| a.name.clone())
                    .or_else(|| env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str).map(host_of)),
                saved: saved.is_some(),
                ..Default::default()
            })
        } else {
            oauth.as_ref().map(|(creds, account)| CurrentLogin {
                kind: Some("oauth".into()),
                email: account.as_ref().and_then(|a| a.get("emailAddress")).and_then(Value::as_str).map(str::to_string),
                name: account.as_ref().and_then(|a| a.get("displayName")).and_then(Value::as_str).map(str::to_string),
                plan_type: creds.get("subscriptionType").and_then(Value::as_str).map(str::to_string),
                saved: find(&oauth_key).is_some(),
            })
        };
        let mut notes = Vec::new();
        if provider.is_some() {
            notes.push("当前 settings.json 里配置了中转供应商，它会盖过官方登录；切到官方账号时会移除这些设置。".to_string());
        }
        let accounts = store
            .accounts
            .iter()
            .map(|a| {
                let key = a.key();
                let is_current = match a.kind {
                    Kind::Provider => provider.is_some() && key == provider_key,
                    Kind::Oauth => provider.is_none() && key.is_some() && key == oauth_key,
                };
                view(a, is_current)
            })
            .collect();
        Ok(AccountsState {
            auth_path: self.claude_home.to_string_lossy().to_string(),
            current,
            current_note: None,
            notes,
            accounts,
        })
    }

    /// 保存当前生效的中转供应商和官方登录（存在的都保存），返回当前生效那个的 id。
    pub fn save_current(&self) -> Result<String, String> {
        let provider = self.live_provider().map(|env| self.upsert_provider_env(None, env)).transpose()?;
        let oauth = match self.live_oauth() {
            Some((creds, Some(account))) => Some(self.upsert_oauth(creds, account)?),
            _ => None,
        };
        provider
            .or(oauth)
            .ok_or_else(|| "当前没有 Claude 登录：没有找到 .credentials.json 里的官方登录，settings.json 里也没有中转供应商".to_string())
    }

    /// 读取官方 `claude auth login` 在独立目录里写出的登录，并保存为账号。
    pub fn import_login_dir(&self, dir: &Path) -> Result<String, String> {
        let creds: Value = account_store::load_json(&dir.join(".credentials.json"))?;
        let oauth = creds.get("claudeAiOauth").cloned().ok_or("登录没有完成：没有生成 .credentials.json")?;
        let account = account_store::load_json::<Value>(&dir.join(".claude.json"))?
            .get("oauthAccount")
            .cloned()
            .ok_or("登录没有完成：缺少账号信息")?;
        self.upsert_oauth(oauth, account)
    }

    fn upsert_oauth(&self, creds: Value, account: Value) -> Result<String, String> {
        let key = oauth_key(&account).ok_or("账号信息不完整（缺少 accountUuid），请重新登录 Claude Code")?;
        let mut store = self.load()?;
        let expires = |c: &Value| c.get("expiresAt").and_then(Value::as_i64).unwrap_or(0);
        let id = match store.accounts.iter_mut().find(|a| a.key().as_ref() == Some(&key)) {
            Some(existing) => {
                if existing.credentials.as_ref().map(expires).unwrap_or(0) <= expires(&creds) {
                    existing.credentials = Some(creds);
                }
                existing.oauth_account = Some(account);
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    kind: Kind::Oauth,
                    credentials: Some(creds),
                    oauth_account: Some(account),
                    name: None,
                    env: Map::new(),
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

    fn upsert_provider_env(&self, name: Option<String>, env: Map<String, Value>) -> Result<String, String> {
        let key = provider_key(&env).ok_or("中转供应商缺少地址和 token")?;
        let mut store = self.load()?;
        let id = match store.accounts.iter_mut().find(|a| a.key().as_ref() == Some(&key)) {
            Some(existing) => {
                existing.env = env;
                if name.is_some() {
                    existing.name = name;
                }
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                let host = env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str).map(host_of);
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    kind: Kind::Provider,
                    credentials: None,
                    oauth_account: None,
                    name: name.or(host),
                    env,
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

    /// 新增或编辑中转供应商，返回 id。
    pub fn upsert_provider(&self, input: ProviderInput) -> Result<String, String> {
        let name = input.name.trim().to_string();
        let base_url = input.base_url.trim().trim_end_matches('/').to_string();
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            return Err("地址必须以 http:// 或 https:// 开头".into());
        }
        let mut store = self.load()?;
        let existing = match &input.id {
            Some(id) => Some(store.accounts.iter().position(|a| &a.id == id && a.kind == Kind::Provider).ok_or("供应商不存在")?),
            None => None,
        };
        let mut env = existing.map(|i| store.accounts[i].env.clone()).unwrap_or_default();
        env.insert("ANTHROPIC_BASE_URL".into(), Value::String(base_url));
        if let Some(token) = input.token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            env.remove("ANTHROPIC_API_KEY");
            env.insert("ANTHROPIC_AUTH_TOKEN".into(), Value::String(token));
        }
        if !env.contains_key("ANTHROPIC_AUTH_TOKEN") && !env.contains_key("ANTHROPIC_API_KEY") {
            return Err("请填写 token".into());
        }
        match input.model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
            Some(model) => env.insert("ANTHROPIC_MODEL".into(), Value::String(model)),
            None => env.remove("ANTHROPIC_MODEL"),
        };
        let key = provider_key(&env);
        if store.accounts.iter().enumerate().any(|(i, a)| Some(i) != existing && a.key() == key) {
            return Err("已有相同地址和 token 的供应商".into());
        }
        let name = if name.is_empty() { None } else { Some(name) };
        let id = match existing {
            Some(i) => {
                let account = &mut store.accounts[i];
                account.env = env;
                account.name = name.or_else(|| account.name.clone());
                account.id.clone()
            }
            None => {
                let id = account_store::new_id();
                let host = env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str).map(host_of);
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    kind: Kind::Provider,
                    credentials: None,
                    oauth_account: None,
                    name: name.or(host),
                    env,
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

    /// 只读地导入 cc-switch 数据库里的 Claude 中转供应商，返回新增数量。
    pub fn import_cc_switch(&self, database: Option<&Path>) -> Result<usize, String> {
        let database = database.ok_or("没有找到 cc-switch 的数据库")?;
        let conn = rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("打开 cc-switch 数据库失败：{}", e))?;
        let mut stmt = conn
            .prepare("SELECT name, settings_config FROM providers WHERE app_type = 'claude'")
            .map_err(|e| format!("读取 cc-switch 供应商失败：{}", e))?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        let before = self.load()?.accounts.len();
        for (name, config) in rows {
            let Ok(config) = serde_json::from_str::<Value>(&config) else { continue };
            let Some(env) = config.get("env").and_then(Value::as_object) else { continue };
            let picked: Map<String, Value> = PROVIDER_KEYS
                .iter()
                .filter_map(|k| env.get(*k).filter(|v| v.as_str().is_some_and(|s| !s.is_empty())).map(|v| (k.to_string(), v.clone())))
                .collect();
            if picked.get("ANTHROPIC_BASE_URL").is_none() || provider_key(&picked).is_none() {
                continue;
            }
            self.upsert_provider_env(Some(name), picked)?;
        }
        Ok(self.load()?.accounts.len() - before)
    }

    /// 切换：先保存当前登录（官方登录的最新 token 也存回），再写入目标账号。
    pub fn switch(&self, id: &str) -> Result<(), String> {
        let _ = self.save_current();
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        match target.kind {
            Kind::Oauth => {
                let creds = target.credentials.clone().ok_or("账号缺少登录信息")?;
                account_store::edit_json_file(&self.credentials_path(), &self.backups, true, |root| {
                    root.insert("claudeAiOauth".into(), creds.clone());
                    Ok(())
                })?;
                if let Some(account) = target.oauth_account.clone() {
                    account_store::edit_json_file(&self.claude_json, &self.backups, true, |root| {
                        root.insert("oauthAccount".into(), account.clone());
                        Ok(())
                    })?;
                }
                if self.live_provider().is_some() {
                    self.edit_env(|env| {
                        for k in PROVIDER_KEYS {
                            env.remove(*k);
                        }
                    })?;
                }
            }
            Kind::Provider => {
                let target_env = target.env.clone();
                self.edit_env(|env| {
                    for k in PROVIDER_KEYS {
                        env.remove(*k);
                    }
                    for (k, v) in &target_env {
                        env.insert(k.clone(), v.clone());
                    }
                })?;
            }
        }
        target.last_used_at = Some(account_store::now());
        self.save(&store)
    }

    /// 修改 `settings.json` 的 `env`，其他设置保持不变；`env` 变空时移除它。
    fn edit_env(&self, change: impl Fn(&mut Map<String, Value>)) -> Result<(), String> {
        account_store::edit_json_file(&self.settings_path(), &self.backups, true, |root| {
            let mut env = root.get("env").and_then(Value::as_object).cloned().unwrap_or_default();
            change(&mut env);
            if env.is_empty() {
                root.remove("env");
            } else {
                root.insert("env".into(), Value::Object(env));
            }
            Ok(())
        })
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
        self.refresh_quota_with(id, &win_http::request, &find_claude_client_id)
    }

    fn refresh_quota_with(&self, id: &str, http: &HttpFn<'_>, client_id: &dyn Fn() -> Option<String>) -> Result<(), String> {
        let store = self.load()?;
        let account = store.accounts.iter().find(|a| a.id == id).ok_or("账号不存在")?;
        if account.kind == Kind::Provider {
            return Err("中转供应商没有统一的额度接口".into());
        }
        let key = account.key();
        let live = self.live_oauth().filter(|(_, a)| a.as_ref().and_then(oauth_key) == key && key.is_some());
        let plan = account.credentials.as_ref().and_then(|c| c.get("subscriptionType")).and_then(Value::as_str).map(str::to_string);

        let result: Result<(Quota, Option<Value>), String> = if let Some((creds, _)) = live {
            // 当前登录用 Claude Code 正在用的 token，从不刷新。
            fetch_usage(&creds, plan.clone(), http).map(|q| (q, None)).map_err(|e| match e {
                UsageError::Unauthorized => "当前登录的 token 已失效，请在 Claude Code 中使用一次让它自动刷新，或重新登录".to_string(),
                UsageError::Other(m) => m,
            })
        } else {
            let mut creds = account.credentials.clone().ok_or("账号缺少登录信息")?;
            let mut refreshed = false;
            let expires = creds.get("expiresAt").and_then(Value::as_i64).unwrap_or(0);
            if expires - EXPIRY_MARGIN_MS < Utc::now().timestamp_millis() {
                creds = refresh(&creds, http, client_id)?;
                refreshed = true;
            }
            match fetch_usage(&creds, plan.clone(), http) {
                Ok(q) => Ok((q, refreshed.then_some(creds))),
                Err(UsageError::Unauthorized) if !refreshed => {
                    let creds = refresh(&creds, http, client_id)?;
                    fetch_usage(&creds, plan, http).map(|q| (q, Some(creds))).map_err(UsageError::message)
                }
                Err(e) => Err(e.message()),
            }
        };

        let mut store = self.load()?;
        let Some(account) = store.accounts.iter_mut().find(|a| a.id == id) else { return Ok(()) };
        let result = result.map(|(quota, creds)| {
            if let Some(creds) = creds {
                account.credentials = Some(creds);
            }
            quota
        });
        account.quota.record(&result);
        self.save(&store)?;
        result.map(|_| ())
    }
}

enum UsageError {
    Unauthorized,
    Other(String),
}

impl UsageError {
    fn message(self) -> String {
        match self {
            UsageError::Unauthorized => "token 已失效，请用这个账号重新登录".into(),
            UsageError::Other(m) => m,
        }
    }
}

fn fetch_usage(creds: &Value, plan: Option<String>, http: &HttpFn<'_>) -> Result<Quota, UsageError> {
    let token = creds.get("accessToken").and_then(Value::as_str).unwrap_or("");
    let bearer = format!("Bearer {}", token);
    let response = http(
        "GET",
        USAGE_URL,
        &[("Authorization", bearer.as_str()), ("anthropic-beta", OAUTH_BETA), ("Accept", "application/json"), ("User-Agent", "claude-cli")],
        None,
    )
    .map_err(UsageError::Other)?;
    match response.status {
        200..=299 => parse_usage(&response.text(), plan).map_err(UsageError::Other),
        401 | 403 => Err(UsageError::Unauthorized),
        status => Err(UsageError::Other(format!("额度接口返回 HTTP {}", status))),
    }
}

/// 解析 `oauth/usage`：顶层 `five_hour` / `seven_day` 等窗口的 `utilization`（百分比），
/// 以及 `limits[]` 中按模型的每周限额（`percent`）。
fn parse_usage(text: &str, plan: Option<String>) -> Result<Quota, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "额度接口返回的不是 JSON".to_string())?;
    let obj = value.as_object().ok_or("额度接口返回的不是对象")?;
    let minutes = |kind: &str| {
        if kind.starts_with("five_hour") {
            Some(300)
        } else if kind.starts_with("seven_day") {
            Some(10080)
        } else {
            None
        }
    };
    let mut windows = Vec::new();
    for (kind, w) in obj {
        let Some(used) = w.get("utilization").and_then(Value::as_f64) else { continue };
        let label = kind.strip_prefix("seven_day_").map(|m| {
            let mut c = m.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        });
        windows.push(QuotaWindow {
            kind: kind.clone(),
            label,
            used_percent: used.clamp(0.0, 100.0),
            window_minutes: minutes(kind),
            resets_at: w.get("resets_at").and_then(Value::as_str).map(str::to_string),
        });
    }
    for limit in value.get("limits").and_then(Value::as_array).into_iter().flatten() {
        if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped") || limit.pointer("/scope/surface").is_some_and(|v| !v.is_null()) {
            continue;
        }
        let (Some(model), Some(percent)) = (
            limit.pointer("/scope/model/display_name").and_then(Value::as_str),
            limit.get("percent").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let kind = format!("seven_day_{}", model.to_lowercase());
        windows.retain(|w| w.kind != kind);
        windows.push(QuotaWindow {
            kind,
            label: Some(model.to_string()),
            used_percent: percent.clamp(0.0, 100.0),
            window_minutes: Some(10080),
            resets_at: limit.get("resets_at").and_then(Value::as_str).map(str::to_string),
        });
    }
    // 5 小时在前，其次每周总额度，再是各模型。
    windows.sort_by_key(|w| (w.window_minutes.unwrap_or(i64::MAX), w.label.is_some(), w.kind.clone()));
    // 超额用量：金额单位是 `currency`（通常 USD）。
    let credits_balance = value.get("extra_usage").filter(|e| e.get("is_enabled").and_then(Value::as_bool) == Some(true)).map(|e| {
        let used = e.get("used_credits").and_then(Value::as_f64).unwrap_or(0.0);
        let currency = e.get("currency").and_then(Value::as_str).unwrap_or("USD");
        match e.get("monthly_limit").and_then(Value::as_f64) {
            Some(limit) => format!("{:.2} / {:.2} {}", used, limit, currency),
            None => format!("{:.2} {}", used, currency),
        }
    });
    Ok(Quota { plan_type: plan, windows, credits_balance })
}

fn refresh(creds: &Value, http: &HttpFn<'_>, client_id: &dyn Fn() -> Option<String>) -> Result<Value, String> {
    let token = creds
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("缺少 refresh token，请用这个账号重新登录")?;
    let client_id = client_id().ok_or("没有在已安装的 Claude Code 中找到 OAuth 参数，无法刷新；请用这个账号重新登录")?;
    let body = json!({ "grant_type": "refresh_token", "refresh_token": token, "client_id": client_id }).to_string();
    let response = http("POST", TOKEN_URL, &[("Content-Type", "application/json"), ("Accept", "application/json")], Some(body.as_bytes()))?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "刷新 token 失败（HTTP {} {}），请用这个账号重新登录",
            response.status,
            account_store::error_code(&response.body)
        ));
    }
    let fresh: Value = serde_json::from_slice(&response.body).map_err(|_| "刷新 token 的响应不是 JSON".to_string())?;
    let mut creds = creds.clone();
    let obj = creds.as_object_mut().ok_or("登录信息格式不对")?;
    if let Some(v) = fresh.get("access_token").and_then(Value::as_str) {
        obj.insert("accessToken".into(), Value::String(v.into()));
    }
    if let Some(v) = fresh.get("refresh_token").and_then(Value::as_str) {
        obj.insert("refreshToken".into(), Value::String(v.into()));
    }
    if let Some(seconds) = fresh.get("expires_in").and_then(Value::as_i64) {
        obj.insert("expiresAt".into(), json!(Utc::now().timestamp_millis() + seconds * 1000));
    }
    Ok(creds)
}

/// 在已安装的 Claude Code（npm 全局安装的 `cli.js` 或独立安装的 `claude.exe`）中读取正式环境的 OAuth client id。
pub fn find_claude_client_id() -> Option<String> {
    const MARKER: &[u8] = br#"MANUAL_REDIRECT_URL:"https://platform.claude.com/oauth/code/callback",CLIENT_ID:""#;
    let mut candidates = Vec::new();
    if let Some(appdata) = dirs::config_dir() {
        candidates.push(appdata.join("npm").join("node_modules").join("@anthropic-ai").join("claude-code").join("cli.js"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("bin").join("claude.exe"));
    }
    candidates.into_iter().filter_map(|p| std::fs::read(p).ok()).find_map(|bytes| {
        let at = bytes.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
        let id = std::str::from_utf8(bytes.get(at..at + 36)?).ok()?;
        (id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| id.to_string())
    })
}

fn view(a: &StoredAccount, is_current: bool) -> AccountView {
    match a.kind {
        Kind::Oauth => {
            let acc = a.oauth_account.as_ref();
            let get = |k: &str| acc.and_then(|v| v.get(k)).and_then(Value::as_str).map(str::to_string);
            let creds = a.credentials.as_ref();
            AccountView {
                id: a.id.clone(),
                kind: Some("oauth".into()),
                email: get("emailAddress"),
                name: get("displayName"),
                account_id: get("accountUuid"),
                plan_type: creds.and_then(|c| c.get("subscriptionType")).and_then(Value::as_str).map(str::to_string),
                detail: get("organizationName"),
                is_current,
                access_expires_at: creds
                    .and_then(|c| c.get("expiresAt"))
                    .and_then(Value::as_i64)
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                    .map(|t| t.to_rfc3339()),
                added_at: a.added_at.clone(),
                last_used_at: a.last_used_at.clone(),
                has_quota: true,
                ..Default::default()
            }
            .with_quota(&a.quota)
        }
        Kind::Provider => {
            let url = a.env.get("ANTHROPIC_BASE_URL").and_then(Value::as_str).unwrap_or("");
            let model = a.env.get("ANTHROPIC_MODEL").and_then(Value::as_str);
            AccountView {
                id: a.id.clone(),
                kind: Some("provider".into()),
                name: a.name.clone(),
                detail: Some(match model {
                    Some(m) => format!("{} · {}", url, m),
                    None => url.to_string(),
                }),
                is_current,
                added_at: a.added_at.clone(),
                last_used_at: a.last_used_at.clone(),
                has_quota: false,
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    struct Sandbox {
        root: PathBuf,
        m: ClaudeAccountManager,
    }
    impl Sandbox {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cv-claude-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join(".claude")).unwrap();
            let m = ClaudeAccountManager::new(&root.join(".claude"), &root.join(".claude.json"), &root.join("data"), &root.join("backups"));
            Self { root, m }
        }
        fn write(&self, rel: &str, v: &Value) {
            fs::write(self.root.join(rel), serde_json::to_string_pretty(v).unwrap()).unwrap();
        }
        fn read(&self, rel: &str) -> Value {
            serde_json::from_slice(&fs::read(self.root.join(rel)).unwrap()).unwrap()
        }
        fn login(&self, email: &str, uuid: &str, access: &str, expires_ms: i64) {
            self.write(".claude/.credentials.json", &json!({ "claudeAiOauth": { "accessToken": access, "refreshToken": format!("rt-{access}"), "expiresAt": expires_ms, "subscriptionType": "max" } }));
            let mut cfg = if self.root.join(".claude.json").is_file() { self.read(".claude.json") } else { json!({ "numStartups": 5, "mcpServers": {} }) };
            cfg["oauthAccount"] = json!({ "accountUuid": uuid, "organizationUuid": "org", "emailAddress": email, "organizationName": "Org" });
            self.write(".claude.json", &cfg);
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn later() -> i64 {
        Utc::now().timestamp_millis() + 3_600_000
    }

    #[test]
    fn oauth_accounts_switch_and_keep_other_config() {
        let s = Sandbox::new();
        s.login("a@x.com", "uuid-a", "at-a", later());
        let id_a = s.m.save_current().unwrap();
        s.login("b@x.com", "uuid-b", "at-b", later());
        let id_b = s.m.save_current().unwrap();
        s.m.switch(&id_a).unwrap();
        assert_eq!(s.read(".claude/.credentials.json")["claudeAiOauth"]["accessToken"], "at-a");
        let cfg = s.read(".claude.json");
        assert_eq!(cfg["oauthAccount"]["emailAddress"], "a@x.com");
        assert_eq!(cfg["numStartups"], 5);
        let state = s.m.state().unwrap();
        assert!(state.accounts.iter().find(|a| a.id == id_a).unwrap().is_current);
        assert_eq!(state.current.unwrap().plan_type.as_deref(), Some("max"));
        s.m.switch(&id_b).unwrap();
        assert_eq!(s.read(".claude/.credentials.json")["claudeAiOauth"]["accessToken"], "at-b");
        assert!(fs::read_dir(s.root.join("backups/accounts/claude")).unwrap().count() >= 2);
    }

    #[test]
    fn providers_replace_env_and_official_switch_removes_them() {
        let s = Sandbox::new();
        s.login("a@x.com", "uuid-a", "at-a", later());
        let id_oauth = s.m.save_current().unwrap();
        s.write(".claude/settings.json", &json!({ "model": "opus", "env": { "ANTHROPIC_BASE_URL": "https://relay.one/", "ANTHROPIC_AUTH_TOKEN": "sk-1", "ANTHROPIC_MODEL": "m1", "OTHER": "keep" } }));
        let id_one = s.m.save_current().unwrap();
        assert_ne!(id_one, id_oauth);
        let state = s.m.state().unwrap();
        assert_eq!(state.current.as_ref().unwrap().kind.as_deref(), Some("provider"));
        assert_eq!(state.notes.len(), 1);
        assert!(!state.accounts.iter().find(|a| a.id == id_oauth).unwrap().is_current);

        let id_two = s.m.upsert_provider(ProviderInput { id: None, name: "Two".into(), base_url: "https://relay.two".into(), token: Some("sk-2".into()), model: None }).unwrap();
        assert!(s.m.upsert_provider(ProviderInput { id: None, name: "Dup".into(), base_url: "https://relay.two".into(), token: Some("sk-2".into()), model: None }).is_err());
        s.m.switch(&id_two).unwrap();
        let settings = s.read(".claude/settings.json");
        assert_eq!(settings["env"], json!({ "OTHER": "keep", "ANTHROPIC_BASE_URL": "https://relay.two", "ANTHROPIC_AUTH_TOKEN": "sk-2" }));
        assert_eq!(settings["model"], "opus");

        // 编辑时 token 留空表示保留原值。
        s.m.upsert_provider(ProviderInput { id: Some(id_two.clone()), name: "Two!".into(), base_url: "https://relay.two".into(), token: None, model: Some("m2".into()) }).unwrap();
        let view = s.m.state().unwrap().accounts.into_iter().find(|a| a.id == id_two).unwrap();
        assert_eq!(view.name.as_deref(), Some("Two!"));
        assert_eq!(view.detail.as_deref(), Some("https://relay.two · m2"));

        s.m.switch(&id_oauth).unwrap();
        assert_eq!(s.read(".claude/settings.json")["env"], json!({ "OTHER": "keep" }));
        assert!(s.m.state().unwrap().accounts.iter().find(|a| a.id == id_oauth).unwrap().is_current);
        assert!(s.m.refresh_quota(&id_one).unwrap_err().contains("中转"));
    }

    #[test]
    fn imports_claude_providers_from_cc_switch_read_only() {
        let s = Sandbox::new();
        let db = s.root.join("cc-switch.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE providers (app_type TEXT, name TEXT, settings_config TEXT);
               INSERT INTO providers VALUES ('claude', 'Relay', '{"env":{"ANTHROPIC_BASE_URL":"https://r.example","ANTHROPIC_AUTH_TOKEN":"sk-r"}}');
               INSERT INTO providers VALUES ('claude', 'Claude Official', '{"env":{}}');
               INSERT INTO providers VALUES ('codex', 'X', '{"auth":{}}');"#,
        )
        .unwrap();
        drop(conn);
        assert_eq!(s.m.import_cc_switch(Some(&db)).unwrap(), 1);
        assert_eq!(s.m.import_cc_switch(Some(&db)).unwrap(), 0);
        assert_eq!(s.m.state().unwrap().accounts[0].name.as_deref(), Some("Relay"));
    }

    const USAGE: &str = r#"{"five_hour":{"utilization":12.0,"resets_at":"2026-10-24T02:00:00Z"},
        "seven_day":{"utilization":40.5,"resets_at":"2026-10-28T00:00:00Z"},
        "seven_day_opus":{"utilization":3,"resets_at":null},
        "limits":[{"kind":"weekly_scoped","group":"weekly","percent":55,"resets_at":"2026-10-28T00:00:00Z","scope":{"model":{"display_name":"Opus"}}}],
        "extra_usage":{"is_enabled":true,"used_credits":9,"monthly_limit":100,"currency":"USD"}}"#;

    #[test]
    fn parses_usage_windows_and_scoped_limits() {
        let q = parse_usage(USAGE, Some("max".into())).unwrap();
        let kinds: Vec<_> = q.windows.iter().map(|w| (w.kind.as_str(), w.used_percent, w.label.as_deref())).collect();
        assert_eq!(kinds, vec![("five_hour", 12.0, None), ("seven_day", 40.5, None), ("seven_day_opus", 55.0, Some("Opus"))]);
        assert_eq!(q.credits_balance.as_deref(), Some("9.00 / 100.00 USD"));
    }

    #[test]
    fn quota_never_refreshes_the_live_login_but_refreshes_saved_ones() {
        let s = Sandbox::new();
        let expired = Utc::now().timestamp_millis() - 10;
        s.login("old@x.com", "uuid-old", "at-old", expired);
        let id_old = s.m.save_current().unwrap();
        s.login("cur@x.com", "uuid-cur", "at-cur", expired);
        let id_cur = s.m.save_current().unwrap();
        let calls = RefCell::new(Vec::<String>::new());
        let http = |method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>| {
            calls.borrow_mut().push(format!("{method} {url}"));
            if url == TOKEN_URL {
                let sent: Value = serde_json::from_slice(body.unwrap()).unwrap();
                assert_eq!((sent["refresh_token"].as_str(), sent["client_id"].as_str()), (Some("rt-at-old"), Some("client-1")));
                let body = json!({ "access_token": "at-new", "refresh_token": "rt-new", "expires_in": 3600 }).to_string();
                return Ok(win_http::HttpResponse { status: 200, body: body.into_bytes() });
            }
            assert!(headers.contains(&("anthropic-beta", OAUTH_BETA)));
            let ok = headers.iter().any(|(k, v)| *k == "Authorization" && *v == "Bearer at-new");
            Ok(win_http::HttpResponse { status: if ok { 200 } else { 401 }, body: USAGE.as_bytes().to_vec() })
        };
        let client = || Some("client-1".to_string());
        s.m.refresh_quota_with(&id_old, &http, &client).unwrap();
        assert_eq!(calls.borrow().len(), 2);
        calls.borrow_mut().clear();
        assert!(s.m.refresh_quota_with(&id_cur, &http, &client).is_err());
        assert_eq!(*calls.borrow(), vec![format!("GET {USAGE_URL}")]);
        assert_eq!(s.read(".claude/.credentials.json")["claudeAiOauth"]["refreshToken"], "rt-at-cur");
        s.m.switch(&id_old).unwrap();
        assert_eq!(s.read(".claude/.credentials.json")["claudeAiOauth"]["refreshToken"], "rt-new");
    }
}
