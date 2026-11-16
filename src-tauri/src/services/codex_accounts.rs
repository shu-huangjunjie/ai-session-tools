//! Codex（ChatGPT OAuth 与第三方 API 中转）多账号：保存、登录、导入、切换、删除与额度查询。

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::account_store::{self, jwt_claims, AccountMeta, AccountView, AccountsState, CurrentLogin, Quota, QuotaState, QuotaWindow};
use super::win_http::{self, HttpFn};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEVICE_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const PKCE_PORT: u16 = 1455;
const PKCE_CALLBACK_PATH: &str = "/auth/callback";
const PKCE_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const PKCE_SCOPES: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";
const LOGIN_TIMEOUT_SECS: u64 = 600;
/// Codex 官方客户端的 OAuth client id（公开值，与 `codex login` 相同）。
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const STORE_FILE: &str = "codex.json";
const MAX_BACKUPS: usize = 30;
/// access token 剩余有效期不足这么多秒就视为过期。
const EXPIRY_MARGIN_SECS: i64 = 120;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[default]
    Oauth,
    Provider,
}

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::Oauth => "oauth",
            Kind::Provider => "provider",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexProviderInput {
    pub id: Option<String>,
    pub name: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexDeviceAuthStart {
    pub device_auth_id: String,
    pub user_code: String,
    pub verification_url: String,
    pub interval_secs: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct CodexTokenImport {
    pub ids: Vec<String>,
    pub failures: Vec<String>,
}

/// 本程序保存的账号（含 token 或 API Key，只存在本机数据目录）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    id: String,
    #[serde(default)]
    kind: Kind,
    #[serde(default)]
    name: Option<String>,
    /// `auth.json` 全文。
    auth: Value,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
    added_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(default, flatten)]
    meta: AccountMeta,
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
    /// `data_dir` 下的 `accounts/codex.json` 存账号，`backups/accounts/codex` 存切换前的 `auth.json` 与 `config.toml`。
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

    fn config_path(&self) -> PathBuf {
        self.codex_home.join("config.toml")
    }

    fn read_live_config_meta(&self) -> (Option<String>, Option<String>) {
        let Ok(text) = fs::read_to_string(self.config_path()) else {
            return (None, None);
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            return (None, None);
        };
        let base_url = doc.get("openai_base_url").and_then(|v| v.as_str()).map(str::to_string);
        let model = doc.get("model").and_then(|v| v.as_str()).map(str::to_string);
        (base_url, model)
    }

    pub fn state(&self) -> Result<AccountsState, String> {
        let store = self.load()?;
        let live = self.read_live();
        let live_id = live.as_ref().and_then(Identity::of);
        let live_key = live_id.as_ref().map(|i| i.key());
        let live_api_key = live
            .as_ref()
            .filter(|v| Identity::of(v).is_none())
            .and_then(|v| v.get("OPENAI_API_KEY").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let (live_base_url, live_model) = self.read_live_config_meta();

        let matched_provider = live_api_key.as_ref().and_then(|k| {
            store.accounts.iter().find(|a| {
                a.kind == Kind::Provider
                    && a.auth.get("OPENAI_API_KEY").and_then(Value::as_str) == Some(k.as_str())
                    && (a.base_url.is_none() || live_base_url.is_none() || a.base_url == live_base_url)
            })
        });

        let current = if let Some(i) = live_id {
            Some(CurrentLogin {
                kind: Some(Kind::Oauth.as_str().into()),
                saved: store.accounts.iter().any(|a| a.kind == Kind::Oauth && Identity::of(&a.auth).map(|x| x.key()) == Some(i.key())),
                email: i.email,
                name: i.name,
                plan_type: i.plan_type,
            })
        } else if let Some(_api_key) = live_api_key.as_ref() {
            Some(CurrentLogin {
                kind: Some(Kind::Provider.as_str().into()),
                saved: matched_provider.is_some(),
                email: None,
                name: matched_provider
                    .and_then(|p| p.name.clone())
                    .or_else(|| live_base_url.clone())
                    .or_else(|| Some("API Key 供应商".into())),
                plan_type: live_model.clone(),
            })
        } else {
            None
        };

        let active_provider_id = matched_provider.map(|p| p.id.as_str());
        Ok(AccountsState {
            auth_path: self.auth_path().to_string_lossy().to_string(),
            current,
            current_note: self.keyring_note(),
            notes: Vec::new(),
            accounts: store
                .accounts
                .iter()
                .map(|a| view(a, live_key.as_deref(), active_provider_id))
                .collect(),
            cockpit_import_count: 0,
            cc_switch_available: false,
        })
    }

    /// 把当前登录保存为账号（已保存则更新它的 token），返回账号 id。
    pub fn save_current(&self) -> Result<String, String> {
        let live = self.read_live().ok_or("当前没有 Codex 登录（~/.codex/auth.json 不存在或无法读取）")?;
        if Identity::of(&live).is_some() {
            return self.upsert(live);
        }
        let api_key = live
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("当前 auth.json 既没有 ChatGPT tokens 也没有 OPENAI_API_KEY，无法保存")?;
        let (base_url, model) = self.read_live_config_meta();
        let name = base_url
            .clone()
            .unwrap_or_else(|| "自定义 OpenAI API".into());
        self.upsert_provider(CodexProviderInput {
            id: None,
            name,
            api_key: api_key.to_string(),
            base_url,
            model,
        })
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

    /// 添加或更新中转 API Key 供应商（兼容 Cockpit Tools Provider + OpenAI API）。
    pub fn upsert_provider(&self, input: CodexProviderInput) -> Result<String, String> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err("请填写供应商名称".into());
        }
        let api_key = input.api_key.trim().to_string();
        if api_key.is_empty() {
            return Err("请填写 API Key".into());
        }
        let base_url = account_store::normalize_note(input.base_url);
        let model = account_store::normalize_note(input.model);
        let auth = json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": api_key,
        });

        let mut store = self.load()?;
        let existing_id = input.id.clone().or_else(|| {
            store.accounts.iter().find(|a| {
                a.kind == Kind::Provider
                    && a.name.as_deref() == Some(name.as_str())
                    && a.base_url == base_url
            }).map(|a| a.id.clone())
        });
        let id = match existing_id.and_then(|eid| store.accounts.iter_mut().find(|a| a.id == eid)) {
            Some(existing) => {
                existing.name = Some(name);
                existing.auth = auth;
                existing.base_url = base_url;
                existing.model = model;
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    kind: Kind::Provider,
                    name: Some(name),
                    auth,
                    base_url,
                    model,
                    added_at: account_store::now(),
                    last_used_at: None,
                    meta: AccountMeta::default(),
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }

    /// 在浏览器中完成 Codex PKCE OAuth 登录（端口 1455，或手动粘贴回调 URL）。
    pub fn login_oauth_pkce(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        take_manual: &dyn Fn() -> Option<String>,
    ) -> Result<String, String> {
        self.login_oauth_pkce_with(on_url, cancelled, take_manual, &win_http::request, &open_in_browser)
    }

    fn login_oauth_pkce_with(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        take_manual: &dyn Fn() -> Option<String>,
        http: &HttpFn<'_>,
        open: &dyn Fn(&str),
    ) -> Result<String, String> {
        let listener = TcpListener::bind(("127.0.0.1", PKCE_PORT)).map_err(|_| {
            format!(
                "本地 OAuth 回调端口 {} 被占用，请先关闭其他登录进程或改用 Device Code 登录",
                PKCE_PORT
            )
        })?;
        let state = uuid::Uuid::new_v4().simple().to_string();
        let (verifier, challenge) = account_store::pkce_pair();
        let url = format!(
            "{}?{}",
            AUTH_URL,
            account_store::form_encode(&[
                ("response_type", "code"),
                ("client_id", CODEX_CLIENT_ID),
                ("redirect_uri", PKCE_REDIRECT_URI),
                ("scope", PKCE_SCOPES),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
                ("id_token_add_organizations", "true"),
                ("codex_cli_simplified_flow", "true"),
                ("state", &state),
                ("originator", "Codex Desktop"),
            ])
        );
        on_url(&url);
        open(&url);
        let code = wait_for_pkce_code(&listener, &state, cancelled, take_manual)?;
        exchange_authorization_code(&code, PKCE_REDIRECT_URI, &verifier, http)
            .and_then(|auth| self.upsert(auth))
    }

    /// OpenAI Device Code 登录第 1 步：申请一次性配对码与验证地址。
    pub fn start_device_login(&self) -> Result<CodexDeviceAuthStart, String> {
        self.start_device_login_with(&win_http::request)
    }

    fn start_device_login_with(&self, http: &HttpFn<'_>) -> Result<CodexDeviceAuthStart, String> {
        let body = json!({ "client_id": CODEX_CLIENT_ID }).to_string();
        let resp = http(
            "POST",
            DEVICE_CODE_URL,
            &[("Content-Type", "application/json"), ("Accept", "application/json")],
            Some(body.as_bytes()),
        )?;
        if !(200..300).contains(&resp.status) {
            return Err(format!(
                "获取 Device Code 失败（HTTP {} {}）",
                resp.status,
                account_store::error_code(&resp.body)
            ));
        }
        let v: Value = serde_json::from_slice(&resp.body).map_err(|_| "Device Code 响应不是 JSON".to_string())?;
        let device_auth_id = v
            .get("device_auth_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("缺少 device_auth_id")?
            .to_string();
        let user_code = v
            .get("user_code")
            .or_else(|| v.get("usercode"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("缺少 user_code")?
            .to_string();
        let interval_secs = v
            .get("interval")
            .and_then(|x| x.as_u64().or_else(|| x.as_str()?.parse::<u64>().ok()))
            .unwrap_or(5)
            .clamp(1, 30);
        Ok(CodexDeviceAuthStart {
            device_auth_id,
            user_code,
            verification_url: DEVICE_VERIFICATION_URL.into(),
            interval_secs,
        })
    }

    /// OpenAI Device Code 登录第 2 步：轮询授权结果并换取 token 保存账号。
    pub fn complete_device_login(
        &self,
        start: &CodexDeviceAuthStart,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        self.complete_device_login_with(start, cancelled, &win_http::request, Duration::from_secs(start.interval_secs))
    }

    fn complete_device_login_with(
        &self,
        start: &CodexDeviceAuthStart,
        cancelled: &dyn Fn() -> bool,
        http: &HttpFn<'_>,
        poll_delay: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(LOGIN_TIMEOUT_SECS);
        let payload = json!({
            "device_auth_id": start.device_auth_id,
            "user_code": start.user_code,
        })
        .to_string();
        loop {
            if cancelled() {
                return Err("已取消登录".into());
            }
            if Instant::now() >= deadline {
                return Err("Device Code 验证已超时，请重新获取配对码".into());
            }
            let resp = http(
                "POST",
                DEVICE_TOKEN_URL,
                &[("Content-Type", "application/json"), ("Accept", "application/json")],
                Some(payload.as_bytes()),
            )?;
            if (200..300).contains(&resp.status) {
                let v: Value = serde_json::from_slice(&resp.body).map_err(|_| "Device Token 响应不是 JSON".to_string())?;
                let code = v
                    .get("authorization_code")
                    .and_then(Value::as_str)
                    .ok_or("响应缺少 authorization_code")?;
                let verifier = v
                    .get("code_verifier")
                    .and_then(Value::as_str)
                    .ok_or("响应缺少 code_verifier")?;
                let auth = exchange_authorization_code(code, DEVICE_REDIRECT_URI, verifier, http)?;
                return self.upsert(auth);
            }
            if matches!(resp.status, 403 | 404) {
                let step = Duration::from_millis(200);
                let mut waited = Duration::ZERO;
                while waited < poll_delay {
                    if cancelled() {
                        return Err("已取消登录".into());
                    }
                    std::thread::sleep(step.min(poll_delay - waited));
                    waited += step;
                }
                continue;
            }
            return Err(format!(
                "Device Code 登录失败（HTTP {} {}）",
                resp.status,
                account_store::error_code(&resp.body)
            ));
        }
    }

    /// 导入 token / auth.json / Cockpit JSON / refresh_token (`rt_...`) 文本，支持批量导入并返回明细。
    pub fn import_tokens(&self, text: &str) -> Result<CodexTokenImport, String> {
        self.import_tokens_with(text, &win_http::request)
    }

    fn import_tokens_with(&self, text: &str, http: &HttpFn<'_>) -> Result<CodexTokenImport, String> {
        let entries = parse_codex_import_entries(text);
        if entries.is_empty() {
            return Err("未识别出有效的 Codex auth.json、refresh_token（rt_...）或 Cockpit 账号 JSON".into());
        }
        let mut result = CodexTokenImport::default();
        for entry in entries {
            let label = entry
                .label
                .clone()
                .unwrap_or_else(|| "Codex 账号".into());
            let outcome: Result<String, String> = match entry.payload {
                CodexImportPayload::FullAuth(auth) => self.upsert_with_meta(auth, entry.meta, entry.plan_type),
                CodexImportPayload::RefreshToken(rt) => refresh_from_rt(&rt, http)
                    .and_then(|auth| self.upsert_with_meta(auth, entry.meta, entry.plan_type)),
                CodexImportPayload::Provider(input) => {
                    self.upsert_provider(input).and_then(|id| {
                        if !entry.meta.tags.is_empty() || entry.meta.note.is_some() {
                            self.update_meta(&id, Some(entry.meta.tags), Some(entry.meta.note))?;
                        }
                        Ok(id)
                    })
                }
            };
            match outcome {
                Ok(id) => result.ids.push(id),
                Err(e) => result.failures.push(format!("{}：{}", label, e)),
            }
        }
        Ok(result)
    }

    /// 从 cc-switch SQLite 数据库（只读）导入 Codex 供应商；跳过官方登录和重复条目，返回导入数量。
    pub fn import_cc_switch(&self, db_path: Option<&Path>) -> Result<usize, String> {
        let owned;
        let path = match db_path {
            Some(p) => p,
            None => {
                owned = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".cc-switch")
                    .join("cc-switch.db");
                &owned
            }
        };
        if !path.is_file() {
            return Ok(0);
        }
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("打开 cc-switch 数据库失败：{}", e))?;
        let mut stmt = conn
            .prepare("SELECT name, settings_config FROM providers WHERE app_type = 'codex'")
            .map_err(|e| format!("读取 cc-switch 供应商失败：{}", e))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| format!("读取 cc-switch 供应商失败：{}", e))?;

        let existing = self.load()?;
        let mut added = 0usize;
        for row in rows {
            let (name, raw) = row.map_err(|e| e.to_string())?;
            let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
            let auth_obj = v.get("auth").unwrap_or(&v);
            let Some(api_key) = auth_obj
                .get("OPENAI_API_KEY")
                .or_else(|| auth_obj.get("api_key"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let cfg_str = v.get("config").and_then(Value::as_str).unwrap_or("");
            let (base_url, model) = cfg_str
                .parse::<toml_edit::DocumentMut>()
                .ok()
                .map(|doc| {
                    (
                        doc.get("openai_base_url").and_then(|x| x.as_str()).map(str::to_string),
                        doc.get("model").and_then(|x| x.as_str()).map(str::to_string),
                    )
                })
                .unwrap_or((None, None));
            let dup = existing.accounts.iter().any(|a| {
                a.kind == Kind::Provider
                    && a.auth.get("OPENAI_API_KEY").and_then(Value::as_str) == Some(api_key)
                    && a.base_url == base_url
            });
            if dup {
                continue;
            }
            self.upsert_provider(CodexProviderInput {
                id: None,
                name,
                api_key: api_key.to_string(),
                base_url,
                model,
            })?;
            added += 1;
        }
        Ok(added)
    }

    pub fn count_cockpit_tools_in(&self, dirs: &[PathBuf]) -> usize {
        let store = self.load().unwrap_or_default();
        collect_cockpit_codex_entries(dirs)
            .into_iter()
            .filter(|e| !is_codex_entry_saved(&store, e))
            .count()
    }

    pub fn import_cockpit_tools_from(&self, dirs: &[PathBuf]) -> Result<CodexTokenImport, String> {
        self.import_cockpit_tools_with(dirs, &win_http::request)
    }

    fn import_cockpit_tools_with(&self, dirs: &[PathBuf], http: &HttpFn<'_>) -> Result<CodexTokenImport, String> {
        let entries = collect_cockpit_codex_entries(dirs);
        if entries.is_empty() {
            return Err("未在本机 Cockpit Tools 目录中找到可导入的 Codex 账号".into());
        }
        let mut result = CodexTokenImport::default();
        for entry in entries {
            let label = entry.label.clone().unwrap_or_else(|| "Codex 账号".into());
            let outcome: Result<String, String> = match entry.payload {
                CodexImportPayload::FullAuth(auth) => self.upsert_with_meta(auth, entry.meta, entry.plan_type),
                CodexImportPayload::RefreshToken(rt) => refresh_from_rt(&rt, http)
                    .and_then(|auth| self.upsert_with_meta(auth, entry.meta, entry.plan_type)),
                CodexImportPayload::Provider(input) => self.upsert_provider(input).and_then(|id| {
                    if !entry.meta.tags.is_empty() || entry.meta.note.is_some() {
                        self.update_meta(&id, Some(entry.meta.tags), Some(entry.meta.note))?;
                    }
                    Ok(id)
                }),
            };
            match outcome {
                Ok(id) => result.ids.push(id),
                Err(e) => result.failures.push(format!("{}：{}", label, e)),
            }
        }
        Ok(result)
    }

    pub fn update_meta(&self, id: &str, tags: Option<Vec<String>>, note: Option<Option<String>>) -> Result<(), String> {
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        target.meta.update(tags, note);
        self.save(&store)
    }

    pub fn batch_update_tags(&self, ids: &[String], add: &[String], remove: &[String]) -> Result<usize, String> {
        let mut store = self.load()?;
        let mut count = 0usize;
        for account in &mut store.accounts {
            if ids.iter().any(|id| id == &account.id) {
                if !add.is_empty() {
                    account.meta.add_tags(add.iter().cloned());
                }
                if !remove.is_empty() {
                    account.meta.remove_tags(remove);
                }
                count += 1;
            }
        }
        self.save(&store)?;
        Ok(count)
    }

    pub fn batch_delete(&self, ids: &[String]) -> Result<usize, String> {
        let mut store = self.load()?;
        let before = store.accounts.len();
        store.accounts.retain(|a| !ids.iter().any(|id| id == &a.id));
        let deleted = before.saturating_sub(store.accounts.len());
        self.save(&store)?;
        Ok(deleted)
    }

    pub fn export_json(&self, ids: Option<&[String]>) -> Result<String, String> {
        let store = self.load()?;
        let selected: Vec<Value> = store
            .accounts
            .iter()
            .filter(|a| ids.is_none_or(|list| list.iter().any(|id| id == &a.id)))
            .map(|a| {
                let id = Identity::of(&a.auth);
                json!({
                    "kind": a.kind.as_str(),
                    "name": a.name.clone().or_else(|| id.as_ref().and_then(|x| x.name.clone())),
                    "email": id.as_ref().and_then(|x| x.email.clone()),
                    "account_id": id.as_ref().and_then(|x| x.account_id.clone()),
                    "plan_type": id.as_ref().and_then(|x| x.plan_type.clone()),
                    "auth": a.auth,
                    "base_url": a.base_url,
                    "model": a.model,
                    "tags": a.meta.tags,
                    "note": a.meta.note,
                    "added_at": a.added_at,
                })
            })
            .collect();
        serde_json::to_string_pretty(&selected).map_err(|e| e.to_string())
    }

    fn upsert(&self, auth: Value) -> Result<String, String> {
        self.upsert_with_meta(auth, AccountMeta::default(), None)
    }

    fn upsert_with_meta(&self, auth: Value, meta: AccountMeta, _plan_hint: Option<String>) -> Result<String, String> {
        let identity = Identity::of(&auth).ok_or("这不是 ChatGPT 账号登录（缺少 tokens），无法保存；如需使用 API Key 请走供应商表单")?;
        if auth.get("tokens").and_then(|t| t.get("refresh_token")).and_then(Value::as_str).unwrap_or("").is_empty() {
            return Err("登录信息缺少 refresh_token，无法保存".into());
        }
        let mut store = self.load()?;
        let key = identity.key();
        let id = match store.accounts.iter_mut().find(|a| a.kind == Kind::Oauth && Identity::of(&a.auth).map(|i| i.key()) == Some(key.clone())) {
            Some(existing) => {
                // 只用更新的 token 覆盖，避免旧备份把 Codex 刷新过的新 token 换回去。
                if newer_than(&auth, &existing.auth) {
                    existing.auth = auth;
                }
                if !meta.tags.is_empty() {
                    existing.meta.add_tags(meta.tags);
                }
                if existing.meta.note.is_none() && meta.note.is_some() {
                    existing.meta.note = meta.note;
                }
                existing.id.clone()
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    kind: Kind::Oauth,
                    name: identity.name,
                    auth,
                    base_url: None,
                    model: None,
                    added_at: Utc::now().to_rfc3339(),
                    last_used_at: None,
                    meta,
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }

    /// 切换到账号：先保存当前登录的最新 token / API Key，备份 `auth.json` / `config.toml`，再写入目标账号。
    pub fn switch(&self, id: &str) -> Result<(), String> {
        if let Some(note) = self.keyring_note() {
            return Err(note);
        }
        if let Some(live) = self.read_live() {
            if Identity::of(&live).is_some() {
                self.save_current()?;
            }
        }
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?.clone();
        let was_provider = self.read_live().is_some_and(|v| Identity::of(&v).is_none());

        let path = self.auth_path();
        if let Ok(bytes) = fs::read(&path) {
            account_store::backup(&self.backups, "auth.json", &bytes, MAX_BACKUPS)?;
        }
        fs::create_dir_all(&self.codex_home).map_err(|e| format!("无法创建 {}：{}", self.codex_home.display(), e))?;
        let text = serde_json::to_string_pretty(&target.auth).map_err(|e| e.to_string())?;
        account_store::atomic_write(&path, text.as_bytes())?;

        // 按账号类型同步 config.toml 中的 openai_base_url / model。
        self.apply_config_toml_on_switch(&target, was_provider)?;

        if let Some(saved_target) = store.accounts.iter_mut().find(|a| a.id == id) {
            saved_target.last_used_at = Some(Utc::now().to_rfc3339());
        }
        self.save(&store)
    }

    fn apply_config_toml_on_switch(&self, target: &StoredAccount, was_provider: bool) -> Result<(), String> {
        let cfg_path = self.config_path();
        if target.kind == Kind::Oauth {
            if was_provider && cfg_path.is_file() {
                if let Ok(bytes) = fs::read(&cfg_path) {
                    let _ = account_store::backup(&self.backups, "config.toml", &bytes, MAX_BACKUPS);
                    let raw = String::from_utf8_lossy(&bytes);
                    if let Ok(mut doc) = raw.parse::<toml_edit::DocumentMut>() {
                        if doc.remove("openai_base_url").is_some() {
                            account_store::atomic_write(&cfg_path, doc.to_string().as_bytes())?;
                        }
                    }
                }
            }
            return Ok(());
        }

        if target.base_url.is_none() && target.model.is_none() && !cfg_path.is_file() {
            return Ok(());
        }
        let existing_bytes = fs::read(&cfg_path).unwrap_or_default();
        if !existing_bytes.is_empty() {
            account_store::backup(&self.backups, "config.toml", &existing_bytes, MAX_BACKUPS)?;
        }
        let raw = String::from_utf8_lossy(&existing_bytes);
        let mut doc = if raw.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            raw.parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析 Codex config.toml 失败：{}", e))?
        };
        match &target.base_url {
            Some(url) => doc["openai_base_url"] = toml_edit::value(url.as_str()),
            None => {
                doc.remove("openai_base_url");
            }
        }
        if let Some(model) = &target.model {
            doc["model"] = toml_edit::value(model.as_str());
        }
        account_store::atomic_write(&cfg_path, doc.to_string().as_bytes())
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

    /// 查询一个账号的额度并保存结果；中转供应商直接报错，官方登录保持刷新隔离。
    pub fn refresh_quota(&self, id: &str) -> Result<(), String> {
        self.refresh_quota_with(id, &win_http::request)
    }

    fn refresh_quota_with(&self, id: &str, http: &HttpFn<'_>) -> Result<(), String> {
        let store = self.load()?;
        let account = store.accounts.iter().find(|a| a.id == id).ok_or("账号不存在")?;
        if account.kind == Kind::Provider {
            return Err("中转供应商无法查询 ChatGPT 官方额度，可用测试连通性检查接口状态".into());
        }
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
        let text = fs::read_to_string(self.config_path()).ok()?;
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

fn view(account: &StoredAccount, live_key: Option<&str>, active_provider_id: Option<&str>) -> AccountView {
    if account.kind == Kind::Provider {
        let mut detail = account.base_url.clone().unwrap_or_else(|| "OpenAI 官方 API".into());
        if let Some(m) = &account.model {
            detail.push_str(&format!(" · {}", m));
        }
        return AccountView {
            id: account.id.clone(),
            kind: Some(Kind::Provider.as_str().into()),
            name: account.name.clone(),
            detail: Some(detail),
            is_current: active_provider_id == Some(account.id.as_str()),
            added_at: account.added_at.clone(),
            last_used_at: account.last_used_at.clone(),
            has_quota: false,
            base_url: account.base_url.clone(),
            model: account.model.clone(),
            ..Default::default()
        }
        .with_meta(&account.meta);
    }
    let identity = Identity::of(&account.auth).unwrap_or_default();
    AccountView {
        id: account.id.clone(),
        kind: Some(Kind::Oauth.as_str().into()),
        is_current: live_key == Some(identity.key().as_str()),
        email: identity.email,
        name: identity.name.or_else(|| account.name.clone()),
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
    .with_meta(&account.meta)
}

fn open_in_browser(url: &str) {
    account_store::open_in_browser(url);
}

fn exchange_authorization_code(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    http: &HttpFn<'_>,
) -> Result<Value, String> {
    let body = account_store::form_encode(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", CODEX_CLIENT_ID),
        ("code_verifier", code_verifier),
    ]);
    let resp = http(
        "POST",
        TOKEN_URL,
        &[
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Accept", "application/json"),
        ],
        Some(body.as_bytes()),
    )?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "用授权码换取 Codex token 失败（HTTP {} {}）",
            resp.status,
            account_store::error_code(&resp.body)
        ));
    }
    let v: Value = serde_json::from_slice(&resp.body).map_err(|_| "token 响应不是有效的 JSON".to_string())?;
    build_auth_from_token_response(&v, None)
}

fn refresh_from_rt(refresh_token: &str, http: &HttpFn<'_>) -> Result<Value, String> {
    let body = serde_json::to_string(&json!({
        "client_id": CODEX_CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "scope": "openid profile email",
    }))
    .map_err(|e| e.to_string())?;
    let resp = http(
        "POST",
        TOKEN_URL,
        &[("Content-Type", "application/json"), ("Accept", "application/json")],
        Some(body.as_bytes()),
    )?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "刷新 Codex refresh_token 失败（HTTP {} {}）",
            resp.status,
            account_store::error_code(&resp.body)
        ));
    }
    let v: Value = serde_json::from_slice(&resp.body).map_err(|_| "刷新响应不是 JSON".to_string())?;
    build_auth_from_token_response(&v, Some(refresh_token))
}

fn build_auth_from_token_response(v: &Value, fallback_rt: Option<&str>) -> Result<Value, String> {
    let access_token = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("响应缺少 access_token")?;
    let refresh_token = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or(fallback_rt)
        .ok_or("响应缺少 refresh_token")?;
    let id_token = v.get("id_token").and_then(Value::as_str).unwrap_or("");
    let id_claims = (!id_token.is_empty()).then(|| jwt_claims(id_token)).flatten().unwrap_or(Value::Null);
    let access_claims = jwt_claims(access_token).unwrap_or(Value::Null);
    let account_id = id_claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .or_else(|| {
            access_claims
                .get("https://api.openai.com/auth")
                .and_then(|a| a.get("chatgpt_account_id"))
                .and_then(Value::as_str)
        });
    Ok(json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": Value::Null,
        "tokens": {
            "id_token": id_token,
            "access_token": access_token,
            "refresh_token": refresh_token,
            "account_id": account_id,
        },
        "last_refresh": Utc::now().to_rfc3339(),
    }))
}

fn wait_for_pkce_code(
    listener: &TcpListener,
    state: &str,
    cancelled: &dyn Fn() -> bool,
    take_manual: &dyn Fn() -> Option<String>,
) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if cancelled() {
            return Err("已取消登录".into());
        }
        if let Some(raw_manual) = take_manual() {
            if let Some(code) = account_store::extract_callback_code(&raw_manual, Some(state))? {
                return Ok(code);
            }
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
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let target = first_line.split_whitespace().nth(1).unwrap_or("");
                let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
                let outcome = if path == PKCE_CALLBACK_PATH {
                    Some(account_store::extract_callback_code(target, Some(state)).and_then(|c| {
                        c.ok_or_else(|| "授权回调里缺少 authorization code".to_string())
                    }))
                } else {
                    None
                };
                let (status, page) = match &outcome {
                    None => ("404 Not Found", String::new()),
                    Some(Ok(_)) => (
                        "200 OK",
                        account_store::oauth_callback_page("授权完成", "Codex 账号已保存到 Context Vault，可关闭此页面。"),
                    ),
                    Some(Err(e)) => ("400 Bad Request", account_store::oauth_callback_page("授权未完成", e)),
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
            Err(e) => return Err(format!("授权回调服务异常：{}", e)),
        }
    }
}

enum CodexImportPayload {
    FullAuth(Value),
    RefreshToken(String),
    Provider(CodexProviderInput),
}

struct CodexImportEntry {
    label: Option<String>,
    plan_type: Option<String>,
    meta: AccountMeta,
    payload: CodexImportPayload,
}

fn entry_from_codex_json(item: &Value) -> Option<CodexImportEntry> {
    let tags = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>())
        .unwrap_or_default();
    let note = item.get("note").or_else(|| item.get("notes")).and_then(Value::as_str).map(str::to_string);
    let meta = AccountMeta::normalized(tags, note);
    let plan_type = item.get("plan_type").and_then(Value::as_str).map(str::to_string);
    let email = item.get("email").and_then(Value::as_str).map(str::to_string);
    let name = item.get("name").and_then(Value::as_str).map(str::to_string);

    // 1) 完整 auth 对象（`{ auth: { tokens: ... } }` 或 `{ tokens: ... }`）。
    let auth_candidate = item.get("auth").filter(|v| v.is_object()).unwrap_or(item);
    if Identity::of(auth_candidate).is_some()
        && auth_candidate
            .get("tokens")
            .and_then(|t| t.get("refresh_token"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    {
        return Some(CodexImportEntry {
            label: email.or(name),
            plan_type,
            meta,
            payload: CodexImportPayload::FullAuth(auth_candidate.clone()),
        });
    }

    // 2) API Key / Provider 形式。
    let api_key = item
        .get("OPENAI_API_KEY")
        .or_else(|| auth_candidate.get("OPENAI_API_KEY"))
        .or_else(|| item.get("api_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let is_provider = item.get("kind").and_then(Value::as_str) == Some("provider")
        || item.get("auth_mode").and_then(Value::as_str) == Some("apikey")
        || (api_key.is_some() && item.get("tokens").is_none());
    if is_provider {
        if let Some(key) = api_key {
            let base_url = item.get("base_url").or_else(|| item.get("openai_base_url")).and_then(Value::as_str).map(str::to_string);
            let model = item.get("model").and_then(Value::as_str).map(str::to_string);
            let prov_name = name
                .clone()
                .or_else(|| email.clone())
                .or_else(|| base_url.clone())
                .unwrap_or_else(|| "API 供应商".into());
            return Some(CodexImportEntry {
                label: Some(prov_name.clone()),
                plan_type,
                meta,
                payload: CodexImportPayload::Provider(CodexProviderInput {
                    id: None,
                    name: prov_name,
                    api_key: key.to_string(),
                    base_url,
                    model,
                }),
            });
        }
    }

    // 3) Cockpit Tools 扁平化 / `tokens` 字段含 refresh_token。
    let tokens = item.get("tokens");
    let refresh_token = item
        .get("refresh_token")
        .or_else(|| item.get("refreshToken"))
        .or_else(|| tokens.and_then(|t| t.get("refresh_token")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(rt) = refresh_token {
        let access_token = item
            .get("access_token")
            .or_else(|| item.get("accessToken"))
            .or_else(|| tokens.and_then(|t| t.get("access_token")))
            .and_then(Value::as_str)
            .unwrap_or("");
        let id_token = item
            .get("id_token")
            .or_else(|| item.get("idToken"))
            .or_else(|| tokens.and_then(|t| t.get("id_token")))
            .and_then(Value::as_str)
            .unwrap_or("");
        let account_id = item
            .get("account_id")
            .or_else(|| tokens.and_then(|t| t.get("account_id")))
            .and_then(Value::as_str);
        if !access_token.is_empty() && !id_token.is_empty() {
            let auth = json!({
                "auth_mode": "chatgpt",
                "OPENAI_API_KEY": Value::Null,
                "tokens": {
                    "id_token": id_token,
                    "access_token": access_token,
                    "refresh_token": rt,
                    "account_id": account_id,
                },
                "last_refresh": Utc::now().to_rfc3339(),
            });
            if Identity::of(&auth).is_some() {
                return Some(CodexImportEntry {
                    label: email.or(name),
                    plan_type,
                    meta,
                    payload: CodexImportPayload::FullAuth(auth),
                });
            }
        }
        return Some(CodexImportEntry {
            label: email.or_else(|| Some(format!("{}…", rt.chars().take(10).collect::<String>()))),
            plan_type,
            meta,
            payload: CodexImportPayload::RefreshToken(rt.to_string()),
        });
    }

    None
}

fn parse_codex_import_entries(text: &str) -> Vec<CodexImportEntry> {
    let mut out = Vec::new();
    let trimmed = text.trim().trim_start_matches('\u{feff}');
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        let items: Vec<&Value> = match &value {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => match o.get("accounts") {
                Some(Value::Array(a)) => a.iter().collect(),
                _ => vec![&value],
            },
            _ => Vec::new(),
        };
        for item in items {
            if let Some(e) = entry_from_codex_json(item) {
                out.push(e);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let pattern = regex::Regex::new(r"rt_[0-9A-Za-z._\-]{16,}").unwrap();
    let mut seen = Vec::new();
    for m in pattern.find_iter(text) {
        let rt = m.as_str().to_string();
        if !seen.contains(&rt) {
            seen.push(rt.clone());
            out.push(CodexImportEntry {
                label: Some(format!("{}…", rt.chars().take(10).collect::<String>())),
                plan_type: None,
                meta: AccountMeta::default(),
                payload: CodexImportPayload::RefreshToken(rt),
            });
        }
    }
    out
}

fn collect_cockpit_codex_entries(dirs: &[PathBuf]) -> Vec<CodexImportEntry> {
    let mut out: Vec<CodexImportEntry> = Vec::new();
    for root in dirs {
        if !root.is_dir() {
            continue;
        }
        let mut index_meta: HashMap<String, AccountMeta> = HashMap::new();
        let index_path = root.join("codex_accounts.json");
        if let Ok(text) = fs::read_to_string(&index_path) {
            if let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
                let arr = v.get("accounts").and_then(Value::as_array).cloned().or_else(|| v.as_array().cloned());
                if let Some(items) = arr {
                    for item in items {
                        let tags = item
                            .get("tags")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>())
                            .unwrap_or_default();
                        let note = item.get("note").or_else(|| item.get("notes")).and_then(Value::as_str).map(str::to_string);
                        let meta = AccountMeta::normalized(tags, note);
                        if let Some(email) = item.get("email").and_then(Value::as_str) {
                            index_meta.insert(email.to_lowercase(), meta.clone());
                        }
                        if let Some(mut entry) = entry_from_codex_json(&item) {
                            if entry.meta.tags.is_empty() {
                                entry.meta.tags = meta.tags;
                            }
                            if entry.meta.note.is_none() {
                                entry.meta.note = meta.note;
                            }
                            out.push(entry);
                        }
                    }
                }
            }
        }
        let acc_dir = root.join("codex_accounts");
        if let Ok(rd) = fs::read_dir(&acc_dir) {
            for file in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")) {
                let Ok(text) = fs::read_to_string(&file) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { continue };
                let Some(mut entry) = entry_from_codex_json(&v) else { continue };
                if let Some(label) = &entry.label {
                    if let Some(m) = index_meta.get(&label.to_lowercase()) {
                        if entry.meta.tags.is_empty() {
                            entry.meta.tags = m.tags.clone();
                        }
                        if entry.meta.note.is_none() {
                            entry.meta.note = m.note.clone();
                        }
                    }
                }
                out.push(entry);
            }
        }
    }
    out
}

fn is_codex_entry_saved(store: &Store, entry: &CodexImportEntry) -> bool {
    match &entry.payload {
        CodexImportPayload::FullAuth(auth) => {
            let Some(key) = Identity::of(auth).map(|i| i.key()) else {
                return false;
            };
            store.accounts.iter().any(|a| a.kind == Kind::Oauth && Identity::of(&a.auth).map(|i| i.key()) == Some(key.clone()))
        }
        CodexImportPayload::RefreshToken(rt) => store.accounts.iter().any(|a| {
            a.auth.get("tokens").and_then(|t| t.get("refresh_token")).and_then(Value::as_str) == Some(rt.as_str())
        }),
        CodexImportPayload::Provider(input) => store.accounts.iter().any(|a| {
            a.kind == Kind::Provider
                && a.auth.get("OPENAI_API_KEY").and_then(Value::as_str) == Some(input.api_key.as_str())
                && a.base_url == input.base_url
        }),
    }
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
    fn saves_api_key_logins_and_blocks_keyring_mode() {
        let s = Sandbox::new();
        s.login(&json!({ "OPENAI_API_KEY": "sk-test", "tokens": null }));
        let pid = s.m.save_current().unwrap();
        let st = s.m.state().unwrap();
        assert_eq!(st.current.as_ref().and_then(|c| c.kind.as_deref()), Some("provider"));
        assert!(st.accounts.iter().any(|a| a.id == pid && a.kind.as_deref() == Some("provider")));
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

    #[test]
    fn device_code_login_and_pkce_manual_callback_work() {
        let s = Sandbox::new();
        let fresh_access = jwt(json!({ "exp": future(), "https://api.openai.com/auth": { "chatgpt_account_id": "acc-dev" } }));
        let fresh_id = jwt(json!({ "email": "dev@x.com", "name": "Dev User", "https://api.openai.com/auth": { "chatgpt_account_id": "acc-dev", "chatgpt_user_id": "u-dev", "chatgpt_plan_type": "team" } }));
        let poll_count = RefCell::new(0u32);
        let http = |_method: &str, url: &str, _headers: &[(&str, &str)], _body: Option<&[u8]>| {
            let reply = |status: u16, body: Value| Ok(win_http::HttpResponse { status, body: body.to_string().into_bytes() });
            if url == DEVICE_CODE_URL {
                return reply(200, json!({ "device_auth_id": "dauth-1", "user_code": "ABCD-1234", "interval": 1 }));
            }
            if url == DEVICE_TOKEN_URL {
                *poll_count.borrow_mut() += 1;
                if *poll_count.borrow() == 1 {
                    return reply(404, json!({ "error": "authorization_pending" }));
                }
                return reply(200, json!({ "authorization_code": "ac-dev", "code_verifier": "cv-dev" }));
            }
            assert_eq!(url, TOKEN_URL);
            reply(200, json!({ "access_token": fresh_access, "refresh_token": "rt_dev_refresh_token_123456", "id_token": fresh_id }))
        };

        let start = s.m.start_device_login_with(&http).unwrap();
        assert_eq!(start.user_code, "ABCD-1234");
        let id = s.m.complete_device_login_with(&start, &|| false, &http, Duration::from_millis(1)).unwrap();
        let state = s.m.state().unwrap();
        let acc = state.accounts.iter().find(|a| a.id == id).unwrap();
        assert_eq!(acc.email.as_deref(), Some("dev@x.com"));
        assert_eq!(acc.plan_type.as_deref(), Some("team"));

        // PKCE 回调等待函数使用随机端口验证，不抢占真实 1455 端口。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let manual = std::sync::Mutex::new(Some(format!("http://localhost:1455/auth/callback?code=c1&state=st9")));
        let code = wait_for_pkce_code(&listener, "st9", &|| false, &|| manual.lock().unwrap().take()).unwrap();
        assert_eq!(code, "c1");
    }

    #[test]
    fn provider_switching_updates_config_toml_and_cockpit_import_preserves_meta() {
        let s = Sandbox::new();
        let oa = auth("oauth@x.com", "acc-oa", future(), "rt_oauth");
        s.login(&oa);
        let id_oauth = s.m.save_current().unwrap();

        let id_prov = s.m.upsert_provider(CodexProviderInput {
            id: None,
            name: "Relay OpenAI".into(),
            api_key: "sk-relay-123".into(),
            base_url: Some("https://relay.example/v1".into()),
            model: Some("gpt-5.3-codex".into()),
        }).unwrap();
        s.m.switch(&id_prov).unwrap();
        assert_eq!(s.live()["auth_mode"], "apikey");
        assert_eq!(s.live()["OPENAI_API_KEY"], "sk-relay-123");
        let toml = fs::read_to_string(s.root.join("codex/config.toml")).unwrap();
        assert!(toml.contains("https://relay.example/v1") && toml.contains("gpt-5.3-codex"));
        let st = s.m.state().unwrap();
        assert!(st.accounts.iter().find(|a| a.id == id_prov).unwrap().is_current);

        // 切回官方 OAuth 时移除中转 base_url。
        s.m.switch(&id_oauth).unwrap();
        let toml_after = fs::read_to_string(s.root.join("codex/config.toml")).unwrap();
        assert!(!toml_after.contains("openai_base_url"));

        // Cockpit Tools 导入 & cc-switch 导入 & 批量标签/删除/导出。
        let cockpit = s.root.join(".cockpit_tools");
        fs::create_dir_all(cockpit.join("codex_accounts")).unwrap();
        let cp_auth = auth("cp@x.com", "acc-cp", future(), "rt_cp");
        fs::write(
            cockpit.join("codex_accounts/a1.json"),
            json!({
                "email": "cp@x.com",
                "tags": ["Cockpit", "Team"],
                "note": "测试备注",
                "tokens": cp_auth["tokens"],
            }).to_string(),
        ).unwrap();
        assert_eq!(s.m.count_cockpit_tools_in(&[cockpit.clone()]), 1);
        let imp = s.m.import_cockpit_tools_with(&[cockpit.clone()], &|_m, _u, _h, _b| unreachable!()).unwrap();
        assert_eq!(imp.ids.len(), 1);
        assert_eq!(s.m.count_cockpit_tools_in(&[cockpit]), 0);

        s.m.update_meta(&imp.ids[0], Some(vec!["主力".into()]), Some(Some("新备注".into()))).unwrap();
        s.m.batch_update_tags(&[imp.ids[0].clone()], &["Codex".into()], &[]).unwrap();
        let exported = s.m.export_json(Some(&[imp.ids[0].clone()])).unwrap();
        assert!(exported.contains("cp@x.com") && exported.contains("新备注") && exported.contains("Codex"));
        assert_eq!(s.m.batch_delete(&[imp.ids[0].clone(), id_prov]).unwrap(), 2);
    }
}
