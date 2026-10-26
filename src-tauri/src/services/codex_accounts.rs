//! Codex（ChatGPT 登录）多账号：保存、导入、切换、删除，以及查询 5 小时 / 每周额度。
//!
//! 每个账号保存一份 `auth.json` 快照，存于本程序数据目录的 `accounts/codex.json`。
//! 切换时先把 `~/.codex/auth.json` 的最新 token 存回它对应的账号（Codex 运行时会自己刷新 token，
//! 不存回就会丢掉新 token），备份后再写入目标账号。
//!
//! 额度来自 `chatgpt.com/backend-api/wham/usage`。当前登录的账号直接用 `auth.json` 的 token，
//! 从不由这里刷新——refresh token 用一次就作废，这里刷新会把正在用的 Codex 挤下线。
//! 其他账号的 access token 过期时，用它保存的 refresh token 向 `auth.openai.com` 换新。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::account_store::{self, jwt_claims, AccountView, AccountsState, CurrentLogin, Quota, QuotaState, QuotaWindow};
use super::win_http;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Codex 官方客户端的 OAuth client id（公开值，与 `codex login` 相同）。
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const STORE_FILE: &str = "codex.json";
const MAX_BACKUPS: usize = 30;
/// access token 剩余有效期不足这么多秒就视为过期。
const EXPIRY_MARGIN_SECS: i64 = 120;

/// 本程序保存的账号（含 token，只存在本机数据目录）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    id: String,
    /// `auth.json` 全文。
    auth: Value,
    added_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(flatten)]
    quota: QuotaState,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    accounts: Vec<StoredAccount>,
}

/// 从 token 解析出的身份。同一个邮箱在不同工作区（Team）是不同账号，所以用 account_id + user_id 区分。
#[derive(Debug, Clone, Default, PartialEq)]
struct Identity {
    email: Option<String>,
    name: Option<String>,
    account_id: Option<String>,
    user_id: Option<String>,
    plan_type: Option<String>,
    access_exp: Option<i64>,
}

impl Identity {
    fn of(auth: &Value) -> Option<Self> {
        let tokens = auth.get("tokens")?;
        let id_claims = tokens.get("id_token").and_then(Value::as_str).and_then(jwt_claims).unwrap_or(Value::Null);
        let access_claims = tokens.get("access_token").and_then(Value::as_str).and_then(jwt_claims).unwrap_or(Value::Null);
        let openai = |claims: &Value, key: &str| {
            claims.get("https://api.openai.com/auth").and_then(|a| a.get(key)).and_then(Value::as_str).map(str::to_string)
        };
        let account_id = tokens
            .get("account_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| openai(&id_claims, "chatgpt_account_id"))
            .or_else(|| openai(&access_claims, "chatgpt_account_id"));
        let user_id = openai(&id_claims, "chatgpt_user_id")
            .or_else(|| openai(&access_claims, "chatgpt_user_id"))
            .or_else(|| id_claims.get("sub").and_then(Value::as_str).map(str::to_string));
        if account_id.is_none() && user_id.is_none() {
            return None;
        }
        Some(Self {
            email: id_claims
                .get("email")
                .and_then(Value::as_str)
                .or_else(|| access_claims.get("https://api.openai.com/profile").and_then(|p| p.get("email")).and_then(Value::as_str))
                .map(str::to_string),
            name: id_claims.get("name").and_then(Value::as_str).map(str::to_string),
            account_id,
            user_id,
            plan_type: openai(&id_claims, "chatgpt_plan_type").or_else(|| openai(&access_claims, "chatgpt_plan_type")),
            access_exp: access_claims.get("exp").and_then(Value::as_i64),
        })
    }

    fn key(&self) -> String {
        format!("{}|{}", self.account_id.as_deref().unwrap_or(""), self.user_id.as_deref().unwrap_or(""))
    }
}

pub struct CodexAccountManager {
    codex_home: PathBuf,
    store_path: PathBuf,
    backups: PathBuf,
}

impl CodexAccountManager {
    /// `data_dir` 下的 `accounts/codex.json` 存账号，`backups/accounts/codex` 存切换前的 `auth.json`。
    pub fn new(codex_home: &Path, data_dir: &Path, backups: &Path) -> Self {
        Self {
            codex_home: codex_home.to_path_buf(),
            store_path: data_dir.join("accounts").join(STORE_FILE),
            backups: backups.join("accounts").join("codex"),
        }
    }

    fn auth_path(&self) -> PathBuf {
        self.codex_home.join("auth.json")
    }

    pub fn state(&self) -> Result<AccountsState, String> {
        let store = self.load()?;
        let live = self.read_live();
        let live_key = live.as_ref().and_then(Identity::of).map(|i| i.key());
        let current = live.as_ref().and_then(Identity::of).map(|i| CurrentLogin {
            saved: store.accounts.iter().any(|a| Identity::of(&a.auth).map(|x| x.key()) == Some(i.key())),
            email: i.email,
            name: i.name,
            plan_type: i.plan_type,
            kind: None,
        });
        Ok(AccountsState {
            auth_path: self.auth_path().to_string_lossy().to_string(),
            current,
            current_note: self.keyring_note(),
            notes: Vec::new(),
            accounts: store
                .accounts
                .iter()
                .map(|a| view(a, live_key.as_deref()))
                .collect(),
        })
    }

    /// 把当前登录保存为账号（已保存则更新它的 token），返回账号 id。
    pub fn save_current(&self) -> Result<String, String> {
        let live = self.read_live().ok_or("当前没有 Codex 登录（~/.codex/auth.json 不存在或无法读取）")?;
        self.upsert(live)
    }

    /// 导入一个 `auth.json` 文件，返回账号 id。
    pub fn import_file(&self, path: &Path) -> Result<String, String> {
        let bytes = fs::read(path).map_err(|e| format!("读取 {} 失败：{}", path.display(), e))?;
        let auth: Value = serde_json::from_slice(&bytes).map_err(|_| format!("{} 不是有效的 auth.json", path.display()))?;
        self.upsert(auth)
    }

    /// 读取 `codex login` 在临时目录里写出的登录，并保存为账号。
    pub fn import_login_dir(&self, dir: &Path) -> Result<String, String> {
        self.import_file(&dir.join("auth.json"))
    }

    fn upsert(&self, auth: Value) -> Result<String, String> {
        let identity = Identity::of(&auth).ok_or("这不是 ChatGPT 账号登录（缺少 tokens），无法保存；API Key 登录没有额度可查")?;
        if auth.get("tokens").and_then(|t| t.get("refresh_token")).and_then(Value::as_str).unwrap_or("").is_empty() {
            return Err("登录信息缺少 refresh_token，无法保存".into());
        }
        let mut store = self.load()?;
        let key = identity.key();
        let id = match store.accounts.iter_mut().find(|a| Identity::of(&a.auth).map(|i| i.key()) == Some(key.clone())) {
            Some(existing) => {
                // 只用更新的 token 覆盖，避免旧备份把 Codex 刷新过的新 token 换回去。
                if newer_than(&auth, &existing.auth) {
                    existing.auth = auth;
                }
                existing.id.clone()
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    auth,
                    added_at: Utc::now().to_rfc3339(),
                    last_used_at: None,
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }

    /// 切换到账号：先保存当前登录的最新 token（未保存的当前登录会自动保存，方便切回），
    /// 备份 `auth.json`，再写入目标账号。
    pub fn switch(&self, id: &str) -> Result<(), String> {
        if let Some(note) = self.keyring_note() {
            return Err(note);
        }
        if self.read_live().as_ref().and_then(Identity::of).is_some() {
            self.save_current()?;
        }
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        let path = self.auth_path();
        if let Ok(bytes) = fs::read(&path) {
            account_store::backup(&self.backups, "auth.json", &bytes, MAX_BACKUPS)?;
        }
        fs::create_dir_all(&self.codex_home).map_err(|e| format!("无法创建 {}：{}", self.codex_home.display(), e))?;
        let text = serde_json::to_string_pretty(&target.auth).map_err(|e| e.to_string())?;
        account_store::atomic_write(&path, text.as_bytes())?;
        target.last_used_at = Some(Utc::now().to_rfc3339());
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

    /// 查询一个账号的额度并保存结果；失败原因记在账号上，同时返回错误。
    pub fn refresh_quota(&self, id: &str) -> Result<(), String> {
        self.refresh_quota_with(id, &win_http::request)
    }

    fn refresh_quota_with(&self, id: &str, http: &HttpFn<'_>) -> Result<(), String> {
        let store = self.load()?;
        let account = store.accounts.iter().find(|a| a.id == id).ok_or("账号不存在")?;
        let key = Identity::of(&account.auth).map(|i| i.key());
        let live = self.read_live();
        let is_current = live.as_ref().and_then(Identity::of).map(|i| i.key()) == key && key.is_some();

        let result = if is_current {
            // 当前账号用 Codex 正在用的 token，从不刷新。
            let auth = live.unwrap();
            fetch_usage(&auth, http).map(|q| (q, None)).map_err(|e| match e {
                UsageError::Unauthorized => "当前登录的 token 已失效，请在 Codex 中重新登录或使用一次让它自动刷新".to_string(),
                UsageError::Other(m) => m,
            })
        } else {
            let mut auth = account.auth.clone();
            let mut refreshed = false;
            if Identity::of(&auth).and_then(|i| i.access_exp).is_some_and(|exp| exp - EXPIRY_MARGIN_SECS < Utc::now().timestamp()) {
                auth = refresh_tokens(&auth, http)?;
                refreshed = true;
            }
            match fetch_usage(&auth, http) {
                Ok(q) => Ok((q, refreshed.then_some(auth))),
                Err(UsageError::Unauthorized) if !refreshed => {
                    let auth = refresh_tokens(&auth, http)?;
                    fetch_usage(&auth, http)
                        .map(|q| (q, Some(auth)))
                        .map_err(|e| e.message())
                }
                Err(e) => Err(e.message()),
            }
        };

        // 网络请求期间文件可能变了，重新读取后只改这一个账号。
        let mut store = self.load()?;
        let Some(account) = store.accounts.iter_mut().find(|a| a.id == id) else { return Ok(()) };
        let result = result.map(|(quota, new_auth)| {
            if let Some(new_auth) = new_auth {
                account.auth = new_auth;
            }
            quota
        });
        account.quota.record(&result);
        let outcome = result.map(|_| ());
        self.save(&store)?;
        outcome
    }

    /// Codex 配置为把凭据存进系统钥匙串时，`auth.json` 不起作用，不能用文件切换。
    fn keyring_note(&self) -> Option<String> {
        let text = fs::read_to_string(self.codex_home.join("config.toml")).ok()?;
        let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        let store = doc.get("cli_auth_credentials_store")?.as_str()?;
        (store != "file").then(|| {
            format!("Codex 配置了 cli_auth_credentials_store = \"{}\"，登录不在 auth.json 里，暂不支持切换", store)
        })
    }

    fn read_live(&self) -> Option<Value> {
        let bytes = fs::read(self.auth_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn load(&self) -> Result<Store, String> {
        account_store::load_json(&self.store_path)
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        account_store::save_json(&self.store_path, &json!({ "version": 1, "accounts": store.accounts }))
    }
}

type HttpFn<'a> = dyn Fn(&str, &str, &[(&str, &str)], Option<&[u8]>) -> Result<win_http::HttpResponse, String> + 'a;

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

fn fetch_usage(auth: &Value, http: &HttpFn<'_>) -> Result<Quota, UsageError> {
    let tokens = auth.get("tokens").ok_or_else(|| UsageError::Other("缺少 tokens".into()))?;
    let access = tokens.get("access_token").and_then(Value::as_str).unwrap_or("");
    let bearer = format!("Bearer {}", access);
    let account_id = Identity::of(auth).and_then(|i| i.account_id).unwrap_or_default();
    let mut headers = vec![("Authorization", bearer.as_str()), ("Accept", "application/json"), ("User-Agent", "codex-cli")];
    if !account_id.is_empty() {
        headers.push(("ChatGPT-Account-Id", account_id.as_str()));
    }
    let response = http("GET", USAGE_URL, &headers, None).map_err(UsageError::Other)?;
    match response.status {
        200..=299 => parse_usage(&response.text()).map_err(UsageError::Other),
        401 | 403 => Err(UsageError::Unauthorized),
        status => Err(UsageError::Other(format!("额度接口返回 HTTP {}", status))),
    }
}

/// 解析 `wham/usage`：`rate_limit.primary_window` / `secondary_window` 的 used_percent 与重置时间。
fn parse_usage(text: &str) -> Result<Quota, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "额度接口返回的不是 JSON".to_string())?;
    let mut windows = Vec::new();
    if let Some(limit) = value.get("rate_limit") {
        for kind in ["primary", "secondary"] {
            let Some(w) = limit.get(format!("{}_window", kind)).filter(|w| w.is_object()) else { continue };
            let Some(used) = w.get("used_percent").and_then(Value::as_f64) else { continue };
            let resets_at = w
                .get("reset_at")
                .and_then(Value::as_i64)
                .or_else(|| w.get("reset_after_seconds").and_then(Value::as_i64).map(|s| Utc::now().timestamp() + s))
                .and_then(|t| Utc.timestamp_opt(t, 0).single())
                .map(|t| t.to_rfc3339());
            windows.push(QuotaWindow {
                kind: kind.to_string(),
                label: None,
                used_percent: used.clamp(0.0, 100.0),
                window_minutes: w.get("limit_window_seconds").and_then(Value::as_i64).map(|s| s / 60),
                resets_at,
            });
        }
    }
    let credits_balance = value.get("credits").filter(|c| c.get("has_credits").and_then(Value::as_bool) == Some(true)).and_then(|c| {
        if c.get("unlimited").and_then(Value::as_bool) == Some(true) {
            Some("unlimited".to_string())
        } else {
            c.get("balance").map(|b| b.as_str().map(str::to_string).unwrap_or_else(|| b.to_string()))
        }
    });
    Ok(Quota {
        plan_type: value.get("plan_type").and_then(Value::as_str).map(str::to_string),
        windows,
        credits_balance,
    })
}

/// 用 refresh token 换新 token，返回更新后的 `auth.json`。
fn refresh_tokens(auth: &Value, http: &HttpFn<'_>) -> Result<Value, String> {
    let refresh = auth
        .get("tokens")
        .and_then(|t| t.get("refresh_token"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("缺少 refresh_token，请用这个账号重新登录")?;
    let body = json!({ "client_id": CODEX_CLIENT_ID, "grant_type": "refresh_token", "refresh_token": refresh }).to_string();
    let response = http(
        "POST",
        TOKEN_URL,
        &[("Content-Type", "application/json"), ("Accept", "application/json")],
        Some(body.as_bytes()),
    )?;
    if !(200..300).contains(&response.status) {
        let code = serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|v| v.get("error").map(|e| e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string())))
            .unwrap_or_default();
        return Err(format!("刷新 token 失败（HTTP {} {}），请用这个账号重新登录", response.status, code));
    }
    let fresh: Value = serde_json::from_slice(&response.body).map_err(|_| "刷新 token 的响应不是 JSON".to_string())?;
    let mut auth = auth.clone();
    let tokens = auth.get_mut("tokens").and_then(Value::as_object_mut).ok_or("缺少 tokens")?;
    for key in ["id_token", "access_token", "refresh_token"] {
        if let Some(v) = fresh.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            tokens.insert(key.to_string(), Value::String(v.to_string()));
        }
    }
    auth["last_refresh"] = Value::String(Utc::now().to_rfc3339());
    Ok(auth)
}

/// 比较两份登录的新旧：按 access token 的签发 / 过期时间，其次按 last_refresh。
fn newer_than(a: &Value, b: &Value) -> bool {
    let exp = |v: &Value| Identity::of(v).and_then(|i| i.access_exp).unwrap_or(0);
    let refreshed = |v: &Value| v.get("last_refresh").and_then(Value::as_str).unwrap_or("").to_string();
    (exp(a), refreshed(a)) >= (exp(b), refreshed(b))
}

fn view(account: &StoredAccount, live_key: Option<&str>) -> AccountView {
    let identity = Identity::of(&account.auth).unwrap_or_default();
    AccountView {
        id: account.id.clone(),
        is_current: live_key == Some(identity.key().as_str()),
        email: identity.email,
        name: identity.name,
        account_id: identity.account_id,
        plan_type: account.quota.quota.as_ref().and_then(|q| q.plan_type.clone()).or(identity.plan_type),
        access_expires_at: identity.access_exp.and_then(|t| Utc.timestamp_opt(t, 0).single()).map(|t| t.to_rfc3339()),
        last_refresh: account.auth.get("last_refresh").and_then(Value::as_str).map(str::to_string),
        added_at: account.added_at.clone(),
        last_used_at: account.last_used_at.clone(),
        has_quota: true,
        ..Default::default()
    }
    .with_quota(&account.quota)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn b64(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    fn jwt(claims: Value) -> String {
        format!("{}.{}.sig", b64(br#"{"alg":"none"}"#), b64(claims.to_string().as_bytes()))
    }

    fn auth(email: &str, account: &str, exp: i64, refresh: &str) -> Value {
        let claims = json!({ "email": email, "https://api.openai.com/auth": { "chatgpt_account_id": account, "chatgpt_user_id": format!("user-{email}"), "chatgpt_plan_type": "plus" } });
        json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": jwt(claims),
                "access_token": jwt(json!({ "exp": exp, "https://api.openai.com/auth": { "chatgpt_account_id": account } })),
                "refresh_token": refresh,
                "account_id": account,
            },
            "last_refresh": "2026-10-01T00:00:00Z",
        })
    }

    struct Sandbox {
        root: PathBuf,
        m: CodexAccountManager,
    }
    impl Sandbox {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cv-accounts-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("codex")).unwrap();
            let m = CodexAccountManager::new(&root.join("codex"), &root.join("data"), &root.join("backups"));
            Self { root, m }
        }
        fn login(&self, auth: &Value) {
            fs::write(self.root.join("codex/auth.json"), serde_json::to_string_pretty(auth).unwrap()).unwrap();
        }
        fn live(&self) -> Value {
            serde_json::from_slice(&fs::read(self.root.join("codex/auth.json")).unwrap()).unwrap()
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn future() -> i64 {
        Utc::now().timestamp() + 3600
    }

    #[test]
    fn decodes_jwt_identity() {
        let id = Identity::of(&auth("a@x.com", "acc-1", 1_900_000_000, "r")).unwrap();
        assert_eq!(id.email.as_deref(), Some("a@x.com"));
        assert_eq!(id.plan_type.as_deref(), Some("plus"));
        assert_eq!(id.access_exp, Some(1_900_000_000));
        assert_eq!(id.key(), "acc-1|user-a@x.com");
        assert!(Identity::of(&json!({ "OPENAI_API_KEY": "sk" })).is_none());
    }

    #[test]
    fn save_switch_keeps_the_newest_tokens_and_backs_up() {
        let s = Sandbox::new();
        let a = auth("a@x.com", "acc-a", future(), "ra");
        let b = auth("b@x.com", "acc-b", future(), "rb");
        s.login(&b);
        let id_b = s.m.save_current().unwrap();
        s.login(&a);
        let id_a = s.m.save_current().unwrap();
        assert_eq!(s.m.save_current().unwrap(), id_a);

        // Codex 刷新了 a 的 token；切到 b 时应把新 token 存回 a。
        let a_refreshed = auth("a@x.com", "acc-a", future() + 100, "ra2");
        s.login(&a_refreshed);
        s.m.switch(&id_b).unwrap();
        assert_eq!(s.live()["tokens"]["refresh_token"], "rb");
        let state = s.m.state().unwrap();
        assert!(state.accounts.iter().find(|x| x.id == id_b).unwrap().is_current);
        assert!(state.current.as_ref().unwrap().saved);
        s.m.switch(&id_a).unwrap();
        assert_eq!(s.live()["tokens"]["refresh_token"], "ra2");
        assert_eq!(fs::read_dir(s.root.join("backups/accounts/codex")).unwrap().count(), 2);

        // 导入旧备份不会把新 token 换回去。
        let file = s.root.join("old-auth.json");
        fs::write(&file, serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(s.m.import_file(&file).unwrap(), id_a);
        s.m.switch(&id_b).unwrap();
        s.m.switch(&id_a).unwrap();
        assert_eq!(s.live()["tokens"]["refresh_token"], "ra2");
    }

    #[test]
    fn unsaved_current_login_is_saved_before_switching_away() {
        let s = Sandbox::new();
        s.login(&auth("b@x.com", "acc-b", future(), "rb"));
        let id_b = s.m.save_current().unwrap();
        s.login(&auth("c@x.com", "acc-c", future(), "rc"));
        s.m.switch(&id_b).unwrap();
        let emails: Vec<_> = s.m.state().unwrap().accounts.into_iter().filter_map(|a| a.email).collect();
        assert_eq!(emails, vec!["b@x.com", "c@x.com"]);
        assert!(s.m.delete(&id_b).is_ok());
        assert!(s.m.delete(&id_b).is_err());
        assert_eq!(s.live()["tokens"]["refresh_token"], "rb");
    }

    #[test]
    fn rejects_api_key_logins_and_keyring_mode() {
        let s = Sandbox::new();
        s.login(&json!({ "OPENAI_API_KEY": "sk-test", "tokens": null }));
        assert!(s.m.save_current().is_err());
        assert!(s.m.state().unwrap().current.is_none());
        fs::write(s.root.join("codex/config.toml"), "cli_auth_credentials_store = \"keyring\"\n").unwrap();
        assert!(s.m.state().unwrap().current_note.is_some());
        assert!(s.m.switch("x").unwrap_err().contains("keyring"));
    }

    const USAGE: &str = r#"{"plan_type":"plus","rate_limit":{"allowed":true,
        "primary_window":{"used_percent":42.5,"limit_window_seconds":18000,"reset_after_seconds":3600,"reset_at":1900000000},
        "secondary_window":{"used_percent":7,"limit_window_seconds":604800,"reset_at":1900500000}},
        "credits":{"has_credits":true,"unlimited":false,"balance":"12.5"}}"#;

    #[test]
    fn parses_usage_windows() {
        let q = parse_usage(USAGE).unwrap();
        assert_eq!(q.plan_type.as_deref(), Some("plus"));
        assert_eq!(q.windows.len(), 2);
        assert_eq!((q.windows[0].kind.as_str(), q.windows[0].used_percent, q.windows[0].window_minutes), ("primary", 42.5, Some(300)));
        assert_eq!(q.windows[1].window_minutes, Some(10080));
        assert!(q.windows[0].resets_at.as_deref().unwrap().starts_with("2030-03-17"));
        assert_eq!(q.credits_balance.as_deref(), Some("12.5"));
        assert!(parse_usage("{}").unwrap().windows.is_empty());
    }

    #[test]
    fn quota_refreshes_expired_inactive_accounts_but_never_the_current_one() {
        let s = Sandbox::new();
        let expired = auth("old@x.com", "acc-old", Utc::now().timestamp() - 10, "r-old");
        s.login(&expired);
        let id_old = s.m.save_current().unwrap();
        s.login(&auth("cur@x.com", "acc-cur", Utc::now().timestamp() - 10, "r-cur"));
        let id_cur = s.m.save_current().unwrap();

        let calls = RefCell::new(Vec::<String>::new());
        let fresh_access = jwt(json!({ "exp": future(), "https://api.openai.com/auth": { "chatgpt_account_id": "acc-old" } }));
        let http = |method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>| {
            calls.borrow_mut().push(format!("{method} {url}"));
            if url == TOKEN_URL {
                let sent: Value = serde_json::from_slice(body.unwrap()).unwrap();
                assert_eq!(sent["refresh_token"], "r-old");
                assert_eq!(sent["client_id"], CODEX_CLIENT_ID);
                let body = json!({ "access_token": fresh_access, "refresh_token": "r-old-2" }).to_string();
                return Ok(win_http::HttpResponse { status: 200, body: body.into_bytes() });
            }
            let auth = headers.iter().find(|(k, _)| *k == "Authorization").unwrap().1;
            let status = if auth.ends_with(&fresh_access) { 200 } else { 401 };
            Ok(win_http::HttpResponse { status, body: USAGE.as_bytes().to_vec() })
        };

        s.m.refresh_quota_with(&id_old, &http).unwrap();
        assert_eq!(*calls.borrow(), vec![format!("POST {TOKEN_URL}"), format!("GET {USAGE_URL}")]);
        let state = s.m.state().unwrap();
        let old = state.accounts.iter().find(|a| a.id == id_old).unwrap();
        assert_eq!(old.quota.as_ref().unwrap().windows.len(), 2);
        assert!(old.quota_error.is_none());

        // 当前账号的 token 失效时只报错，不调用刷新接口。
        calls.borrow_mut().clear();
        assert!(s.m.refresh_quota_with(&id_cur, &http).unwrap_err().contains("Codex"));
        assert_eq!(*calls.borrow(), vec![format!("GET {USAGE_URL}")]);
        assert_eq!(s.live()["tokens"]["refresh_token"], "r-cur");
        let cur = s.m.state().unwrap().accounts.into_iter().find(|a| a.id == id_cur).unwrap();
        assert!(cur.quota_error.is_some());

        // 新 token 已保存：切过去后 auth.json 用的是刷新后的 refresh token。
        s.m.switch(&id_old).unwrap();
        assert_eq!(s.live()["tokens"]["refresh_token"], "r-old-2");
    }
}
