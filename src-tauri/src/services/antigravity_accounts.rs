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

use super::account_store::{self, AccountMeta, AccountView, AccountsState, CurrentLogin, Quota, QuotaState, QuotaWindow};
use super::win_http::{self, HttpFn};


/// 属于账号的 `ItemTable` 键（旧版 Antigravity `state.vscdb`）。
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
const CLOUD_CODE_ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];
const USER_AGENT: &str = "antigravity/2.25.0 (aidev_client; os_type=windows; arch=amd64; cl=1003475615)";
const STORE_FILE: &str = "antigravity.json";
const DB_BACKUPS: usize = 10;

const WIN_CRED_TARGET: &str = "gemini:antigravity";
const WIN_CRED_USER: &str = "antigravity";
const CRED_MAX_BYTES: usize = 2560;
const CRED_FILE_PRIMARY: &str = "jetski-standalone-oauth-token";
const CRED_FILE_CLI_DIR: &str = "antigravity-cli";
const CRED_FILE_CLI_TOKEN: &str = "antigravity-oauth-token";

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

/// Windows 凭据管理器读写抽象，测试中用内存实现保持完全隔离。
pub trait CredStore: Send + Sync {
    fn is_enabled(&self) -> bool {
        true
    }
    fn read_secret(&self) -> Result<Option<String>, String>;
    fn write_secret(&self, payload: &str) -> Result<(), String>;
}

struct NullCredStore;

impl CredStore for NullCredStore {
    fn is_enabled(&self) -> bool {
        false
    }
    fn read_secret(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn write_secret(&self, _payload: &str) -> Result<(), String> {
        Ok(())
    }
}

struct WinCredStore;

#[cfg(windows)]
mod win_cred_ffi {
    use core::ffi::c_void;
    use std::ptr;

    const CRED_TYPE_GENERIC: u32 = 1;
    const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
    const ERROR_NOT_FOUND: i32 = 1168;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct CredentialW {
        flags: u32,
        cred_type: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: FileTime,
        credential_blob_size: u32,
        credential_blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut c_void,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredReadW(target_name: *const u16, cred_type: u32, flags: u32, credential: *mut *mut CredentialW) -> i32;
        fn CredWriteW(credential: *const CredentialW, flags: u32) -> i32;
        fn CredFree(buffer: *const c_void);
    }

    fn wide_z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read(target: &str) -> Result<Option<String>, String> {
        let target_w = wide_z(target);
        let mut ptr: *mut CredentialW = ptr::null_mut();
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) };
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_NOT_FOUND) {
                return Ok(None);
            }
            return Err(format!("读取 Windows 凭据管理器失败：{}", err));
        }
        struct Guard(*mut CredentialW);
        impl Drop for Guard {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { CredFree(self.0.cast()) };
                }
            }
        }
        let _guard = Guard(ptr);
        let cred = unsafe { &*ptr };
        if cred.credential_blob.is_null() || cred.credential_blob_size == 0 {
            return Ok(None);
        }
        let bytes = unsafe { core::slice::from_raw_parts(cred.credential_blob, cred.credential_blob_size as usize) };
        let text = String::from_utf8_lossy(bytes).trim().to_string();
        Ok((!text.is_empty()).then_some(text))
    }

    pub fn write(target: &str, user: &str, payload: &str) -> Result<(), String> {
        let mut target_w = wide_z(target);
        let mut user_w = wide_z(user);
        let mut bytes = payload.as_bytes().to_vec();
        let cred = CredentialW {
            flags: 0,
            cred_type: CRED_TYPE_GENERIC,
            target_name: target_w.as_mut_ptr(),
            comment: ptr::null_mut(),
            last_written: FileTime { low: 0, high: 0 },
            credential_blob_size: bytes.len() as u32,
            credential_blob: bytes.as_mut_ptr(),
            persist: CRED_PERSIST_LOCAL_MACHINE,
            attribute_count: 0,
            attributes: ptr::null_mut(),
            target_alias: ptr::null_mut(),
            user_name: user_w.as_mut_ptr(),
        };
        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            return Err(format!("写入 Windows 凭据管理器失败：{}", std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

impl CredStore for WinCredStore {
    fn is_enabled(&self) -> bool {
        cfg!(windows)
    }

    fn read_secret(&self) -> Result<Option<String>, String> {
        #[cfg(windows)]
        {
            win_cred_ffi::read(WIN_CRED_TARGET)
        }
        #[cfg(not(windows))]
        {
            Ok(None)
        }
    }

    fn write_secret(&self, payload: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            win_cred_ffi::write(WIN_CRED_TARGET, WIN_CRED_USER, payload)
        }
        #[cfg(not(windows))]
        {
            let _ = payload;
            Ok(())
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
    /// 登录项是按 token 合成的（浏览器授权、导入或 2.0 CLI），还没有旧版 `state.vscdb` 快照。
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
    id_token: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    added_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(default, flatten)]
    meta: AccountMeta,
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

struct LiveLogin {
    email: Option<String>,
    name: Option<String>,
    access_token: String,
    refresh_token: String,
    expires_at: Option<i64>,
    id_token: Option<String>,
    db_items: Option<Map<String, Value>>,
}

pub struct AntigravityAccountManager {
    db: PathBuf,
    store_path: PathBuf,
    backups: PathBuf,
    gemini_home: Option<PathBuf>,
    cred_store: Box<dyn CredStore>,
}

impl AntigravityAccountManager {
    /// 测试隔离构造方法：不触碰系统 Windows 凭据管理器与真实 `~/.gemini`。
    pub fn new(db: &Path, data_dir: &Path, backups: &Path) -> Self {
        Self::new_with_cred_store(db, data_dir, backups, None, Box::new(NullCredStore))
    }

    /// 生产环境构造方法：同时支持 Antigravity 2.0（Windows 凭据管理器 + `~/.gemini`）与 1.x `state.vscdb`。
    pub fn new_live(db: &Path, data_dir: &Path, backups: &Path, gemini_home: Option<PathBuf>) -> Self {
        Self::new_with_cred_store(db, data_dir, backups, gemini_home, Box::new(WinCredStore))
    }

    pub fn new_with_cred_store(
        db: &Path,
        data_dir: &Path,
        backups: &Path,
        gemini_home: Option<PathBuf>,
        cred_store: Box<dyn CredStore>,
    ) -> Self {
        Self {
            db: db.to_path_buf(),
            store_path: data_dir.join("accounts").join(STORE_FILE),
            backups: backups.join("accounts").join("antigravity"),
            gemini_home,
            cred_store,
        }
    }

    fn has_v2_target(&self) -> bool {
        if self.cred_store.is_enabled() {
            return true;
        }
        self.gemini_home.as_ref().is_some_and(|home| {
            home.is_dir()
                || home.join(CRED_FILE_PRIMARY).is_file()
                || home.join(CRED_FILE_CLI_DIR).is_dir()
        })
    }

    fn load(&self) -> Result<Store, String> {
        account_store::load_json(&self.store_path)
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        account_store::save_json(&self.store_path, &json!({ "version": 1, "accounts": store.accounts }))
    }

    /// 读取旧版 `state.vscdb` 中的登录快照。
    fn read_live_db(&self) -> Result<Option<Map<String, Value>>, String> {
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

    fn read_live_v2(&self) -> Result<Option<LiveLogin>, String> {
        let mut raw_candidates: Vec<String> = Vec::new();
        if let Some(secret) = self.cred_store.read_secret()? {
            raw_candidates.push(secret);
        }
        if let Some(home) = &self.gemini_home {
            for rel in [
                home.join(CRED_FILE_PRIMARY),
                home.join(CRED_FILE_CLI_DIR).join(CRED_FILE_CLI_TOKEN),
            ] {
                if let Ok(text) = std::fs::read_to_string(&rel) {
                    if !text.trim().is_empty() {
                        raw_candidates.push(text);
                    }
                }
            }
        }
        for raw in raw_candidates {
            if let Some(parsed) = parse_v2_credential_json(&raw) {
                return Ok(Some(parsed));
            }
        }
        Ok(None)
    }

    fn read_live_login(&self, store: &Store) -> Result<Option<LiveLogin>, String> {
        if let Some(mut live_v2) = self.read_live_v2()? {
            if live_v2.email.is_none() {
                if let Some(matched) = store.accounts.iter().find(|a| {
                    token_info(&a.items)
                        .is_some_and(|(_, r, _)| r == live_v2.refresh_token)
                }) {
                    live_v2.email = Some(matched.email.clone());
                    live_v2.name = matched.name.clone();
                } else if let Ok(Some(db_items)) = self.read_live_db() {
                    if token_info(&db_items).is_some_and(|(_, r, _)| r == live_v2.refresh_token) {
                        if let Some((email, name)) = identity(db_items.get(AUTH_KEY)) {
                            live_v2.email = Some(email);
                            live_v2.name = name;
                        }
                        live_v2.db_items = Some(db_items);
                    }
                }
            }
            return Ok(Some(live_v2));
        }

        let Some(items) = self.read_live_db()? else {
            return Ok(None);
        };
        let (email, name) = identity(items.get(AUTH_KEY))
            .map(|(e, n)| (Some(e), n))
            .unwrap_or((None, None));
        let Some((access_token, refresh_token, expires_at)) = token_info(&items) else {
            return Ok(None);
        };
        Ok(Some(LiveLogin {
            email,
            name,
            access_token,
            refresh_token,
            expires_at,
            id_token: None,
            db_items: Some(items),
        }))
    }

    fn auth_path_label(&self) -> String {
        if self.db.is_file() {
            return self.db.to_string_lossy().to_string();
        }
        if self.cred_store.is_enabled() {
            return format!("Windows 凭据管理器 ({})", WIN_CRED_TARGET);
        }
        if let Some(home) = &self.gemini_home {
            return home.join(CRED_FILE_PRIMARY).to_string_lossy().to_string();
        }
        self.db.to_string_lossy().to_string()
    }

    pub fn state(&self) -> Result<AccountsState, String> {
        let store = self.load()?;
        let live = self.read_live_login(&store).unwrap_or(None);
        let live_email = live.as_ref().and_then(|l| l.email.as_ref()).map(|e| e.to_lowercase());
        let live_rt = live.as_ref().map(|l| l.refresh_token.clone());
        let matched_stored = live.as_ref().and_then(|l| {
            store.accounts.iter().find(|a| {
                live_email
                    .as_deref()
                    .is_some_and(|e| a.email.eq_ignore_ascii_case(e))
                    || token_info(&a.items).is_some_and(|(_, r, _)| r == l.refresh_token)
            })
        });
        let current = live.map(|l| {
            let email = l.email.or_else(|| matched_stored.map(|a| a.email.clone()));
            let name = l.name.or_else(|| matched_stored.and_then(|a| a.name.clone()));
            CurrentLogin {
                saved: matched_stored.is_some(),
                email,
                name,
                ..Default::default()
            }
        });
        let current_note = (!self.db.is_file() && !self.has_v2_target())
            .then(|| format!("没有找到 Antigravity 的数据库或凭据存储：{}", self.db.display()));
        Ok(AccountsState {
            auth_path: self.auth_path_label(),
            current,
            current_note,
            notes: Vec::new(),
            accounts: store
                .accounts
                .iter()
                .map(|a| {
                    let is_cur = live_email
                        .as_deref()
                        .is_some_and(|e| a.email.eq_ignore_ascii_case(e))
                        || live_rt.as_deref().is_some_and(|rt| {
                            token_info(&a.items).is_some_and(|(_, r, _)| r == rt)
                        });
                    view(a, is_cur)
                })
                .collect(),
            cockpit_import_count: 0,
            cc_switch_available: false,
        })
    }

    /// 保存当前登录（已保存则用最新数据覆盖），返回账号 id。
    pub fn save_current(&self) -> Result<String, String> {
        self.save_current_with(&win_http::request)
    }

    fn save_current_with(&self, http: &HttpFn<'_>) -> Result<String, String> {
        let mut store = self.load()?;
        let live = self.read_live_login(&store)?.ok_or("当前没有 Antigravity 登录")?;
        let (email, name) = match live.email.clone() {
            Some(email) => (email, live.name.clone()),
            None => fetch_google_profile(&live.access_token, http)
                .ok_or("Antigravity 当前登录缺少邮箱信息，请重试或通过 OAuth 重新授权")?,
        };
        let expires_at = live.expires_at.unwrap_or_else(|| Utc::now().timestamp() + 3600);
        let id = match store.accounts.iter_mut().find(|a| a.email.eq_ignore_ascii_case(&email)) {
            Some(existing) => {
                if let Some(db_items) = live.db_items {
                    if existing.items != db_items {
                        existing.items = db_items;
                    }
                    existing.synthetic = false;
                } else {
                    existing.items = synth_items(
                        &email,
                        name.as_deref().or(existing.name.as_deref()),
                        &live.access_token,
                        &live.refresh_token,
                        expires_at,
                        existing.project_id.as_deref(),
                    );
                }
                existing.access_token = Some(live.access_token);
                existing.access_expires_at = live.expires_at;
                if live.id_token.is_some() {
                    existing.id_token = live.id_token;
                }
                if name.is_some() {
                    existing.name = name;
                }
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                let synthetic = live.db_items.is_none();
                let items = live.db_items.unwrap_or_else(|| {
                    synth_items(
                        &email,
                        name.as_deref(),
                        &live.access_token,
                        &live.refresh_token,
                        expires_at,
                        None,
                    )
                });
                store.accounts.push(StoredAccount {
                    id: id.clone(),
                    email,
                    name,
                    items,
                    synthetic,
                    client_id: None,
                    access_token: Some(live.access_token),
                    access_expires_at: live.expires_at,
                    id_token: live.id_token,
                    project_id: None,
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

    /// 切换账号。若使用旧版 IDE 数据库，调用方须先确认 Antigravity 已关闭。
    pub fn switch(&self, id: &str) -> Result<(), String> {
        self.switch_with(id, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn switch_with(&self, id: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<(), String> {
        let has_db = self.db.is_file();
        let has_v2 = self.has_v2_target();
        if !has_db && !has_v2 {
            return Err(format!("没有找到 Antigravity 的数据库或凭据存储：{}", self.db.display()));
        }
        {
            let store = self.load()?;
            if self.read_live_login(&store)?.is_some() {
                let _ = self.save_current_with(http);
            }
        }
        let mut store = self.load()?;
        let target = store.accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        let (mut access_for_v2, refresh_for_v2, mut exp_for_v2) =
            token_info(&target.items).unwrap_or_else(|| {
                (
                    target.access_token.clone().unwrap_or_default(),
                    String::new(),
                    target.access_expires_at,
                )
            });

        let needs_v2_refresh = has_v2 && exp_for_v2.is_none_or(|exp| exp - 60 <= Utc::now().timestamp());
        if target.synthetic || needs_v2_refresh {
            if !refresh_for_v2.is_empty() {
                if let Ok((client_id, access, expires_at)) = refresh(&refresh_for_v2, target.client_id.as_deref(), http, clients) {
                    access_for_v2 = access.clone();
                    exp_for_v2 = Some(expires_at);
                    target.items = synth_items(&target.email, target.name.as_deref(), &access, &refresh_for_v2, expires_at, target.project_id.as_deref());
                    target.client_id = Some(client_id);
                    target.access_token = Some(access);
                    target.access_expires_at = Some(expires_at);
                }
            }
        }

        // Antigravity 2.0+：写入 Windows 凭据管理器 + `~/.gemini` 令牌文件。
        if has_v2 {
            if refresh_for_v2.is_empty() && access_for_v2.is_empty() {
                return Err("该账号缺少 OAuth token，请重新登录或导入".into());
            }
            let payload = build_credential_payload(
                &access_for_v2,
                &refresh_for_v2,
                exp_for_v2.unwrap_or_else(|| Utc::now().timestamp() + 3600),
                target.id_token.as_deref(),
            )?;
            if self.cred_store.is_enabled() {
                self.cred_store.write_secret(&payload)?;
            }
            if let Some(home) = &self.gemini_home {
                let primary = home.join(CRED_FILE_PRIMARY);
                let _ = account_store::backup_file(&self.backups, &primary);
                account_store::atomic_write(&primary, payload.as_bytes())?;

                let cli_dir = home.join(CRED_FILE_CLI_DIR);
                if cli_dir.is_dir() {
                    let cli_token = cli_dir.join(CRED_FILE_CLI_TOKEN);
                    let _ = account_store::backup_file(&self.backups, &cli_token);
                    account_store::atomic_write(&cli_token, payload.as_bytes())?;
                }
            }
        }

        // Antigravity 1.x 旧版：若存在 state.vscdb，同时写入 SQLite ItemTable。
        if has_db {
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
        }

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
                let (access, refresh, exp) = token_info(&a.items).unwrap_or((String::new(), String::new(), None));
                json!({
                    "email": a.email,
                    "name": a.name,
                    "refresh_token": refresh,
                    "access_token": a.access_token.clone().unwrap_or(access),
                    "expiry_timestamp": a.access_expires_at.or(exp),
                    "project_id": a.project_id,
                    "id_token": a.id_token,
                    "tags": a.meta.tags,
                    "note": a.meta.note,
                    "added_at": a.added_at,
                })
            })
            .collect();
        serde_json::to_string_pretty(&selected).map_err(|e| e.to_string())
    }

    pub fn count_cockpit_tools_in(&self, dirs: &[PathBuf]) -> usize {
        let existing_emails: Vec<String> = self
            .load()
            .map(|s| s.accounts.into_iter().map(|a| a.email.to_lowercase()).collect())
            .unwrap_or_default();
        collect_cockpit_antigravity_entries(dirs)
            .into_iter()
            .filter(|e| {
                e.email
                    .as_ref()
                    .is_none_or(|mail| !existing_emails.contains(&mail.to_lowercase()))
            })
            .count()
    }

    pub fn import_cockpit_tools_from(&self, dirs: &[PathBuf]) -> Result<TokenImport, String> {
        self.import_cockpit_tools_with(dirs, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn import_cockpit_tools_with(
        &self,
        dirs: &[PathBuf],
        http: &HttpFn<'_>,
        clients: &dyn Fn() -> Vec<(String, String)>,
    ) -> Result<TokenImport, String> {
        let entries = collect_cockpit_antigravity_entries(dirs);
        if entries.is_empty() {
            return Err("未在本机 Cockpit Tools 目录中找到可导入的 Antigravity 账号".into());
        }
        let mut result = TokenImport::default();
        for entry in entries {
            let label = entry.email.clone().unwrap_or_else(|| mask_token(&entry.refresh_token));
            let outcome = match refresh(&entry.refresh_token, None, http, clients) {
                Ok((client_id, access, expires_at)) => {
                    self.add_parsed_entry(&client_id, &access, expires_at, None, &entry, http)
                }
                Err(err) if !err.contains("invalid_grant") && entry.email.is_some() => {
                    // 无网络或离线导出测试时，只要 Cockpit 记录已含邮箱与 refresh_token 仍保留导入。
                    let access = entry.access_token.clone().unwrap_or_default();
                    let expires = entry.expiry_timestamp.unwrap_or_else(|| Utc::now().timestamp() + 3600);
                    let client_id = clients().into_iter().next().map(|(id, _)| id).unwrap_or_default();
                    self.add_parsed_entry(&client_id, &access, expires, None, &entry, http)
                }
                Err(err) => Err(err),
            };
            match outcome {
                Ok(id) => result.ids.push(id),
                Err(e) => result.failures.push(format!("{}：{}", label, e)),
            }
        }
        Ok(result)
    }

    pub fn refresh_quota(&self, id: &str) -> Result<(), String> {
        self.refresh_quota_with(id, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn refresh_quota_with(&self, id: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<(), String> {
        let store = self.load()?;
        let account = store.accounts.iter().find(|a| a.id == id).ok_or("账号不存在")?.clone();
        let mut updated = account.clone();
        let outcome: Result<Quota, (String, Option<String>)> = (|| {
            let mut force_refresh = false;
            for attempt in 0..2 {
                let token = match (&updated.access_token, updated.access_expires_at) {
                    (Some(t), Some(exp)) if !force_refresh && exp - 60 > Utc::now().timestamp() => t.clone(),
                    _ => {
                        let (_, refresh_token, _) = token_info(&updated.items)
                            .ok_or_else(|| ("账号里没有 token，请在「添加账号」里重新授权这个账号".to_string(), None))?;
                        let (client_id, access, expires_at) =
                            refresh(&refresh_token, updated.client_id.as_deref(), http, clients)
                                .map_err(|e| (e, None))?;
                        updated.client_id = Some(client_id);
                        updated.access_token = Some(access.clone());
                        updated.access_expires_at = Some(expires_at);
                        access
                    }
                };
                match load_code_assist(&token, http) {
                    Ok((project, plan, credits)) => {
                        if project.is_some() {
                            updated.project_id = project.clone();
                        }
                        let mut quota = fetch_models(&token, project.or(updated.project_id.clone()), http)?;
                        quota.plan_type = plan;
                        if quota.credits_balance.is_none() {
                            quota.credits_balance = credits;
                        }
                        return Ok(quota);
                    }
                    Err((msg, None)) if attempt == 0 && msg.starts_with("token 已失效") => {
                        force_refresh = true;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(("刷新额度失败，请重试".to_string(), None))
        })();

        let mut store = self.load()?;
        let Some(stored) = store.accounts.iter_mut().find(|a| a.id == id) else { return Ok(()) };
        stored.client_id = updated.client_id;
        stored.access_token = updated.access_token;
        stored.access_expires_at = updated.access_expires_at;
        stored.project_id = updated.project_id;
        let (record_res, v_url) = match &outcome {
            Ok(q) => (Ok(q.clone()), None),
            Err((msg, url)) => (Err(msg.clone()), url.clone()),
        };
        stored.quota.record_with_verification(&record_res, v_url);
        self.save(&store)?;
        outcome.map(|_| ()).map_err(|(msg, _)| msg)
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
        return Err("未找到可用的 Google OAuth 参数，请配置 oauth-clients.local.json 或安装 Antigravity 后重试".into());
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

fn parse_google_validation_error(body: &[u8]) -> (String, Option<String>) {
    let fallback = account_store::error_code(body);
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return (fallback, None);
    };
    let err = v.get("error");
    let details = err.and_then(|e| e.get("details")).and_then(Value::as_array);
    let mut url: Option<String> = None;
    let mut reason: Option<String> = None;
    if let Some(arr) = details {
        for d in arr {
            let r = d.get("reason").and_then(Value::as_str);
            if matches!(r, Some("VALIDATION_REQUIRED") | Some("SUBSCRIPTION_REQUIRED")) {
                reason = r.map(str::to_string);
                if let Some(meta) = d.get("metadata") {
                    url = ["validation_url", "verification_url", "appeal_url"]
                        .iter()
                        .find_map(|k| meta.get(*k).and_then(Value::as_str))
                        .map(str::to_string);
                }
                if url.is_some() {
                    break;
                }
            }
        }
    }
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(&fallback);
    if url.is_some() || reason.as_deref() == Some("VALIDATION_REQUIRED") || message.contains("Verify your account") {
        let friendly = if url.is_some() {
            "Google 账号需要人机安全验证，请点击「去验证」完成后重新刷新额度".to_string()
        } else {
            format!("Google 账号受限（{}）", message)
        };
        return (friendly, url);
    }
    (fallback, None)
}

fn cloud_code_post(path: &str, token: &str, body: &Value, http: &HttpFn<'_>) -> Result<Value, (String, Option<String>)> {
    let bearer = format!("Bearer {}", token);
    let body_str = body.to_string();
    let headers = [
        ("Authorization", bearer.as_str()),
        ("Content-Type", "application/json"),
        ("Accept", "application/json"),
        ("User-Agent", USER_AGENT),
    ];
    let mut last_err: Option<(String, Option<String>)> = None;
    for (idx, base) in CLOUD_CODE_ENDPOINTS.iter().enumerate() {
        let url = format!("{}/v1internal:{}", base, path);
        let response = match http("POST", &url, &headers, Some(body_str.as_bytes())) {
            Ok(r) => r,
            Err(e) => {
                last_err = Some((e, None));
                continue;
            }
        };
        match response.status {
            200..=299 => {
                return serde_json::from_slice(&response.body)
                    .map_err(|_| (format!("{} 返回的不是 JSON", path), None));
            }
            401 => return Err(("token 已失效，请在「添加账号」里重新授权这个账号".into(), None)),
            429 | 500..=599 if idx + 1 < CLOUD_CODE_ENDPOINTS.len() => {
                let (code, v_url) = parse_google_validation_error(&response.body);
                last_err = Some((format!("{} 返回 HTTP {} {}", path, response.status, code), v_url));
                continue;
            }
            status => {
                let (code, v_url) = parse_google_validation_error(&response.body);
                let msg = if v_url.is_some() {
                    code
                } else {
                    format!("{} 返回 HTTP {} {}", path, status, code)
                };
                return Err((msg, v_url));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| (format!("{} 请求失败", path), None)))
}

fn extract_credits(v: &Value) -> Option<String> {
    let arr = v
        .get("availableCredits")
        .or_else(|| v.get("available_credits"))
        .and_then(Value::as_array)?;
    let mut total: f64 = 0.0;
    let mut count = 0usize;
    for item in arr {
        let amt = item
            .get("creditAmount")
            .or_else(|| item.get("credit_amount"))
            .or_else(|| item.get("amount"));
        let val = amt.and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse::<f64>().ok()));
        if let Some(n) = val {
            total += n;
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    if (total.fract()).abs() < 1e-6 {
        Some(format!("{} credits", total.round() as i64))
    } else {
        Some(format!("{:.2} credits", total))
    }
}

/// 返回 `(项目 id, 套餐名, credits)`。
fn load_code_assist(token: &str, http: &HttpFn<'_>) -> Result<(Option<String>, Option<String>, Option<String>), (String, Option<String>)> {
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
    let credits = extract_credits(&v);
    Ok((project, tier("paidTier").or_else(|| tier("currentTier")), credits))
}

fn fetch_models(token: &str, project: Option<String>, http: &HttpFn<'_>) -> Result<Quota, (String, Option<String>)> {
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
    Quota {
        plan_type: None,
        windows,
        credits_balance: extract_credits(v),
    }
}

/// Google OAuth client 列表：保留本机客户端中的参数，并追加不随源码上传的本地配置。
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
        if let Some(configured) = super::oauth_client_config::configured_client() {
            if !out.contains(&configured) {
                out.push(configured);
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
            .or(a.access_expires_at)
            .and_then(|t| Utc.timestamp_opt(t, 0).single())
            .map(|t| t.to_rfc3339()),
        added_at: a.added_at.clone(),
        last_used_at: a.last_used_at.clone(),
        has_quota: true,
        ..Default::default()
    }
    .with_quota(&a.quota)
    .with_meta(&a.meta)
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
        self.login_in_browser_with_manual(on_url, cancelled, &|| None)
    }

    pub fn login_in_browser_with_manual(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        take_manual: &dyn Fn() -> Option<String>,
    ) -> Result<String, String> {
        self.login_in_browser_with_full(
            on_url,
            cancelled,
            take_manual,
            &win_http::request,
            &|| oauth_clients().to_vec(),
            &open_in_browser,
        )
    }

    #[cfg(test)]
    fn login_in_browser_with(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        http: &HttpFn<'_>,
        clients: &dyn Fn() -> Vec<(String, String)>,
        open: &dyn Fn(&str),
    ) -> Result<String, String> {
        self.login_in_browser_with_full(on_url, cancelled, &|| None, http, clients, open)
    }

    fn login_in_browser_with_full(
        &self,
        on_url: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
        take_manual: &dyn Fn() -> Option<String>,
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
        let (verifier, challenge) = account_store::pkce_pair();
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
        let code = wait_for_code(&listener, &state, cancelled, take_manual)?;
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
        let id_token = v.get("id_token").and_then(Value::as_str);
        let expires_at = Utc::now().timestamp() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
        let entry = ParsedTokenEntry {
            refresh_token: refresh_token.to_string(),
            ..Default::default()
        };
        self.add_parsed_entry(&client_id, access, expires_at, id_token, &entry, http)
    }

    /// 导入 refresh token：每行一个，或账号 JSON（数组、单个对象或 Cockpit Tools 导出格式）。
    pub fn import_tokens(&self, text: &str) -> Result<TokenImport, String> {
        self.import_tokens_with(text, &win_http::request, &|| oauth_clients().to_vec())
    }

    fn import_tokens_with(&self, text: &str, http: &HttpFn<'_>, clients: &dyn Fn() -> Vec<(String, String)>) -> Result<TokenImport, String> {
        let entries = parse_token_entries(text);
        if entries.is_empty() {
            return Err("没有找到 refresh token：请粘贴以 1// 开头的 token，或含 refresh_token 的账号 JSON".into());
        }
        let mut result = TokenImport::default();
        for entry in entries {
            let label = entry.email.clone().unwrap_or_else(|| mask_token(&entry.refresh_token));
            let outcome = refresh(&entry.refresh_token, None, http, clients)
                .and_then(|(client_id, access, expires_at)| self.add_parsed_entry(&client_id, &access, expires_at, None, &entry, http));
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
            return Err("未找到可用的 Google OAuth 参数，请配置 oauth-clients.local.json 或安装 Antigravity 后重试".into());
        }
        let store = self.load().ok();
        let used = store.as_ref().and_then(|s| s.accounts.iter().find_map(|a| a.client_id.clone()));
        let live = store
            .as_ref()
            .and_then(|s| self.read_live_login(s).ok().flatten())
            .map(|l| l.refresh_token);
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

    /// 用一组 token 新建账号，或更新同邮箱的账号；同时合并标签/备注/项目元数据。
    fn add_parsed_entry(
        &self,
        client_id: &str,
        access: &str,
        expires_at: i64,
        id_token: Option<&str>,
        entry: &ParsedTokenEntry,
        http: &HttpFn<'_>,
    ) -> Result<String, String> {
        let profile = if !access.is_empty() {
            fetch_google_profile(access, http)
        } else {
            None
        };
        let (prof_email, prof_name) = profile.unwrap_or_else(|| (String::new(), None));
        let email = if !prof_email.is_empty() {
            prof_email
        } else {
            entry
                .email
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .ok_or("无法确认这个 token 属于哪个 Google 账号，请在账号 JSON 里写上 email")?
                .to_string()
        };
        let name = prof_name.or_else(|| entry.name.clone());
        let id_tok = id_token.map(str::to_string).or_else(|| entry.id_token.clone());
        let mut store = self.load()?;
        let id = match store.accounts.iter_mut().find(|a| a.email.eq_ignore_ascii_case(&email)) {
            Some(existing) => {
                if existing.project_id.is_none() && entry.project_id.is_some() {
                    existing.project_id = entry.project_id.clone();
                }
                existing.items = synth_items(
                    &email,
                    name.as_deref().or(existing.name.as_deref()),
                    access,
                    &entry.refresh_token,
                    expires_at,
                    existing.project_id.as_deref(),
                );
                existing.synthetic = true;
                if name.is_some() {
                    existing.name = name;
                }
                if !client_id.is_empty() {
                    existing.client_id = Some(client_id.to_string());
                }
                existing.access_token = (!access.is_empty()).then(|| access.to_string());
                existing.access_expires_at = Some(expires_at);
                if id_tok.is_some() {
                    existing.id_token = id_tok;
                }
                if !entry.meta.tags.is_empty() {
                    existing.meta.add_tags(entry.meta.tags.iter().cloned());
                }
                if existing.meta.note.is_none() && entry.meta.note.is_some() {
                    existing.meta.note = entry.meta.note.clone();
                }
                existing.id.clone()
            }
            None => {
                let id = account_store::new_id();
                store.accounts.push(StoredAccount {
                    items: synth_items(
                        &email,
                        name.as_deref(),
                        access,
                        &entry.refresh_token,
                        expires_at,
                        entry.project_id.as_deref(),
                    ),
                    id: id.clone(),
                    email,
                    name,
                    synthetic: true,
                    client_id: (!client_id.is_empty()).then(|| client_id.to_string()),
                    access_token: (!access.is_empty()).then(|| access.to_string()),
                    access_expires_at: Some(expires_at),
                    id_token: id_tok,
                    project_id: entry.project_id.clone(),
                    added_at: account_store::now(),
                    last_used_at: None,
                    meta: entry.meta.clone(),
                    quota: QuotaState::default(),
                });
                id
            }
        };
        self.save(&store)?;
        Ok(id)
    }
}

/// 等浏览器回到本机端口（或用户在 UI 手动粘贴回调链接），取出授权码；其他请求（如 favicon）回 404 后继续等。
fn wait_for_code(
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

#[derive(Debug, Clone, Default)]
struct ParsedTokenEntry {
    refresh_token: String,
    email: Option<String>,
    name: Option<String>,
    project_id: Option<String>,
    id_token: Option<String>,
    access_token: Option<String>,
    expiry_timestamp: Option<i64>,
    meta: AccountMeta,
}

fn fetch_google_profile(access_token: &str, http: &HttpFn<'_>) -> Option<(String, Option<String>)> {
    let bearer = format!("Bearer {}", access_token);
    let r = http("GET", USERINFO_URL, &[("Authorization", bearer.as_str()), ("Accept", "application/json")], None).ok()?;
    if !(200..300).contains(&r.status) {
        return None;
    }
    let v = serde_json::from_slice::<Value>(&r.body).ok()?;
    let email = v.get("email").and_then(Value::as_str)?.trim().to_string();
    if email.is_empty() {
        return None;
    }
    let name = v.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Some((email, name))
}

/// 构造 Antigravity 2.0 Windows 凭据管理器 / CLI token JSON，超过 2560 字节上限时裁剪 `id_token`。
fn build_credential_payload(
    access_token: &str,
    refresh_token: &str,
    expiry_ts: i64,
    id_token: Option<&str>,
) -> Result<String, String> {
    let expiry_iso = Utc
        .timestamp_opt(expiry_ts.max(0), 0)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let mut obj = serde_json::Map::new();
    obj.insert(
        "token".into(),
        json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "refresh_token": refresh_token,
            "expiry": expiry_iso,
        }),
    );
    obj.insert("auth_method".into(), json!("consumer"));
    if let Some(idt) = id_token.map(str::trim).filter(|s| !s.is_empty()) {
        obj.insert("id_token".into(), json!(idt));
    }
    let text = serde_json::to_string(&Value::Object(obj.clone())).map_err(|e| e.to_string())?;
    if text.len() <= CRED_MAX_BYTES {
        return Ok(text);
    }
    obj.remove("id_token");
    serde_json::to_string(&Value::Object(obj)).map_err(|e| e.to_string())
}

fn parse_v2_credential_json(raw: &str) -> Option<LiveLogin> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let token_obj = v.get("token").unwrap_or(&v);
    let refresh_token = ["refresh_token", "refreshToken"]
        .iter()
        .find_map(|k| token_obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let access_token = ["access_token", "accessToken"]
        .iter()
        .find_map(|k| token_obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let expires_at = token_obj
        .get("expiry")
        .or_else(|| token_obj.get("expiry_timestamp"))
        .and_then(|val| {
            val.as_i64().or_else(|| {
                val.as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp()))
            })
        });
    let id_token = v.get("id_token").and_then(Value::as_str).map(str::to_string);
    let claims = id_token.as_deref().and_then(account_store::jwt_claims);
    let email = claims
        .as_ref()
        .and_then(|c| c.get("email").and_then(Value::as_str))
        .or_else(|| v.get("email").and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let name = claims
        .as_ref()
        .and_then(|c| c.get("name").and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(LiveLogin {
        email,
        name,
        access_token,
        refresh_token,
        expires_at,
        id_token,
        db_items: None,
    })
}

fn entry_from_json_value(item: &Value) -> Option<ParsedTokenEntry> {
    let token_obj = item.get("token");
    let refresh_token = ["refresh_token", "refreshToken"]
        .iter()
        .find_map(|k| {
            item.get(*k)
                .or_else(|| token_obj.and_then(|t| t.get(*k)))
        })
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if refresh_token.is_empty() {
        return None;
    }
    let email = item
        .get("email")
        .or_else(|| token_obj.and_then(|t| t.get("email")))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let project_id = item
        .get("project_id")
        .or_else(|| token_obj.and_then(|t| t.get("project_id")))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let access_token = ["access_token", "accessToken"]
        .iter()
        .find_map(|k| {
            item.get(*k)
                .or_else(|| token_obj.and_then(|t| t.get(*k)))
        })
        .and_then(Value::as_str)
        .map(str::to_string);
    let expiry_timestamp = item
        .get("expiry_timestamp")
        .or_else(|| token_obj.and_then(|t| t.get("expiry_timestamp")))
        .and_then(Value::as_i64);
    let id_token = item.get("id_token").and_then(Value::as_str).map(str::to_string);
    let tags = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let note = item.get("note").or_else(|| item.get("notes")).and_then(Value::as_str).map(str::to_string);
    Some(ParsedTokenEntry {
        refresh_token,
        email,
        name,
        project_id,
        id_token,
        access_token,
        expiry_timestamp,
        meta: AccountMeta::normalized(tags, note),
    })
}

fn parse_token_entries(text: &str) -> Vec<ParsedTokenEntry> {
    fn push(out: &mut Vec<ParsedTokenEntry>, entry: ParsedTokenEntry) {
        let token = entry.refresh_token.trim();
        if !token.is_empty() && !out.iter().any(|e| e.refresh_token == token) {
            out.push(entry);
        }
    }
    let mut out: Vec<ParsedTokenEntry> = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(text.trim().trim_start_matches('\u{feff}')) {
        let items: Vec<&Value> = match &value {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => match o.get("accounts") {
                Some(Value::Array(a)) => a.iter().collect(),
                _ => vec![&value],
            },
            _ => Vec::new(),
        };
        for item in items {
            if let Some(entry) = entry_from_json_value(item) {
                push(&mut out, entry);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let pattern = regex::Regex::new(r"1//[0-9A-Za-z_\-]{20,}").unwrap();
    for m in pattern.find_iter(text) {
        push(
            &mut out,
            ParsedTokenEntry {
                refresh_token: m.as_str().to_string(),
                ..Default::default()
            },
        );
    }
    out
}

/// 扫描 `~/.cockpit_tools` 下的 Antigravity 账号文件（`accounts/*.json` 与 `accounts.json`）。
fn collect_cockpit_antigravity_entries(dirs: &[PathBuf]) -> Vec<ParsedTokenEntry> {
    let mut out: Vec<ParsedTokenEntry> = Vec::new();
    let mut push_unique = |entry: ParsedTokenEntry| {
        if let Some(existing) = out.iter_mut().find(|e| {
            e.refresh_token == entry.refresh_token
                || (e.email.is_some() && e.email.as_deref().map(|s| s.to_lowercase()) == entry.email.as_deref().map(|s| s.to_lowercase()))
        }) {
            if !entry.meta.tags.is_empty() {
                existing.meta.add_tags(entry.meta.tags);
            }
            if existing.meta.note.is_none() {
                existing.meta.note = entry.meta.note;
            }
            return;
        }
        out.push(entry);
    };

    for root in dirs {
        if !root.is_dir() {
            continue;
        }
        // 读取索引文件里的 tags/note 或内嵌账号。
        let mut index_meta: HashMap<String, AccountMeta> = HashMap::new();
        let index_path = root.join("accounts.json");
        if let Ok(text) = std::fs::read_to_string(&index_path) {
            if let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
                if let Some(arr) = v.get("accounts").and_then(Value::as_array) {
                    for item in arr {
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
                        if let Some(mut entry) = entry_from_json_value(item) {
                            if entry.meta.tags.is_empty() {
                                entry.meta.tags = meta.tags;
                            }
                            if entry.meta.note.is_none() {
                                entry.meta.note = meta.note;
                            }
                            push_unique(entry);
                        }
                    }
                }
            }
        }
        let accounts_dir = root.join("accounts");
        if let Ok(rd) = std::fs::read_dir(&accounts_dir) {
            for file in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|ext| ext == "json")) {
                let Ok(text) = std::fs::read_to_string(&file) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { continue };
                let Some(mut entry) = entry_from_json_value(&v) else { continue };
                if let Some(mail) = &entry.email {
                    if let Some(m) = index_meta.get(&mail.to_lowercase()) {
                        if entry.meta.tags.is_empty() {
                            entry.meta.tags = m.tags.clone();
                        }
                        if entry.meta.note.is_none() {
                            entry.meta.note = m.note.clone();
                        }
                    }
                }
                push_unique(entry);
            }
        }
    }
    out
}

/// 粘贴的内容里的 `(refresh_token, email)`：账号 JSON 优先，否则按 `1//…` 逐个取出。重复的只算一次。
#[cfg_attr(not(test), allow(dead_code))]
fn parse_token_input(text: &str) -> Vec<(String, Option<String>)> {
    parse_token_entries(text)
        .into_iter()
        .map(|e| (e.refresh_token, e.email))
        .collect()
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

    #[test]
    fn antigravity_v2_credential_store_saves_switches_and_truncates_id_token() {
        let root = std::env::temp_dir().join(format!("cv-ag-v2-{}", uuid::Uuid::new_v4()));
        let gemini_home = root.join(".gemini");
        fs::create_dir_all(gemini_home.join(CRED_FILE_CLI_DIR)).unwrap();

        let cred_mem = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        struct MemCred(std::sync::Arc<std::sync::Mutex<Option<String>>>);
        impl CredStore for MemCred {
            fn is_enabled(&self) -> bool { true }
            fn read_secret(&self) -> Result<Option<String>, String> { Ok(self.0.lock().unwrap().clone()) }
            fn write_secret(&self, payload: &str) -> Result<(), String> {
                *self.0.lock().unwrap() = Some(payload.to_string());
                Ok(())
            }
        }

        // 构造含 email 的 JWT id_token，验证 >2560 字节时自动省略 id_token。
        let big_claim = "x".repeat(2600);
        let small_id_token = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            account_store::base64_url_encode(br#"{"email":"v2a@gmail.com","name":"V2 User"}"#)
        );
        let big_id_token = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            account_store::base64_url_encode(format!(r#"{{"email":"v2a@gmail.com","pad":"{}"}}"#, big_claim).as_bytes())
        );
        let future_exp = Utc::now().timestamp() + 3600;
        let small_payload = build_credential_payload("ya29.v2a", "1//v2a", future_exp, Some(&small_id_token)).unwrap();
        assert!(small_payload.len() <= CRED_MAX_BYTES && small_payload.contains("id_token"));
        let trimmed_payload = build_credential_payload("ya29.v2a", "1//v2a", future_exp, Some(&big_id_token)).unwrap();
        assert!(trimmed_payload.len() <= CRED_MAX_BYTES && !trimmed_payload.contains("id_token"));

        *cred_mem.lock().unwrap() = Some(small_payload);
        let m = AntigravityAccountManager::new_with_cred_store(
            &root.join("missing.vscdb"),
            &root.join("data"),
            &root.join("backups"),
            Some(gemini_home.clone()),
            Box::new(MemCred(cred_mem.clone())),
        );
        let st = m.state().unwrap();
        assert_eq!(st.current.as_ref().and_then(|c| c.email.as_deref()), Some("v2a@gmail.com"));
        assert!(st.current_note.is_none());

        let id_a = m.save_current().unwrap();
        // 再登录另一个 2.0 账号，切换到 a 时同时写 Windows 凭据与两个 ~/.gemini 文件。
        let b_id_token = format!(
            "eyJhbGciOiJub25lIn0.{}.sig",
            account_store::base64_url_encode(br#"{"email":"v2b@gmail.com","name":"V2 B"}"#)
        );
        *cred_mem.lock().unwrap() = Some(build_credential_payload("ya29.v2b", "1//v2b", future_exp, Some(&b_id_token)).unwrap());
        let id_b = m.save_current().unwrap();
        assert_ne!(id_a, id_b);

        m.switch(&id_a).unwrap();
        let active_raw = cred_mem.lock().unwrap().clone().unwrap();
        assert!(active_raw.contains("1//v2a"));
        assert!(fs::read_to_string(gemini_home.join(CRED_FILE_PRIMARY)).unwrap().contains("1//v2a"));
        assert!(fs::read_to_string(gemini_home.join(CRED_FILE_CLI_DIR).join(CRED_FILE_CLI_TOKEN)).unwrap().contains("1//v2a"));
        assert!(m.state().unwrap().accounts.iter().find(|a| a.id == id_a).unwrap().is_current);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_cockpit_tools_with_tags_notes_batch_ops_and_validation_url() {
        let s = Sandbox::new();
        let cockpit = s.root.join(".cockpit_tools");
        fs::create_dir_all(cockpit.join("accounts")).unwrap();
        fs::write(
            cockpit.join("accounts.json"),
            r#"{"accounts":[{"email":"cockpit@gmail.com","tags":["主力","Google Pro"],"note":"来自 Cockpit"}]}"#,
        )
        .unwrap();
        fs::write(
            cockpit.join("accounts").join("acc1.json"),
            r#"{"email":"cockpit@gmail.com","name":"Cockpit User","token":{"refresh_token":"1//good-cockpit-token-012345","access_token":"ya29.cached","project_id":"proj-c"}}"#,
        )
        .unwrap();

        assert_eq!(s.m.count_cockpit_tools_in(&[cockpit.clone()]), 1);
        let calls = RefCell::new(Vec::new());
        let http = fake_google("cockpit@gmail.com", &calls);
        let imported = s.m.import_cockpit_tools_with(&[cockpit.clone()], &http, &clients).unwrap();
        assert_eq!(imported.ids.len(), 1);
        assert_eq!(s.m.count_cockpit_tools_in(&[cockpit]), 0);

        let id = imported.ids[0].clone();
        let acc = s.m.state().unwrap().accounts.into_iter().next().unwrap();
        assert_eq!(acc.tags, vec!["主力", "Google Pro"]);
        assert_eq!(acc.note.as_deref(), Some("来自 Cockpit"));

        s.m.update_meta(&id, Some(vec!["已验证".into()]), Some(Some("更新备注".into()))).unwrap();
        s.m.batch_update_tags(&[id.clone()], &["备用".into()], &["已验证".into()]).unwrap();
        let exported = s.m.export_json(Some(&[id.clone()])).unwrap();
        assert!(exported.contains("cockpit@gmail.com") && exported.contains("备用") && exported.contains("更新备注"));

        // 模拟 Cloud Code 返回 VALIDATION_REQUIRED，记录 verification_url。
        let verify_http = |method: &str, url: &str, _headers: &[(&str, &str)], _body: Option<&[u8]>| {
            if url == TOKEN_URL {
                return Ok(win_http::HttpResponse { status: 200, body: br#"{"access_token":"ya29.v","expires_in":3600}"#.to_vec() });
            }
            if method == "POST" && url.contains("loadCodeAssist") {
                return Ok(win_http::HttpResponse {
                    status: 403,
                    body: br#"{"error":{"code":403,"message":"Verify your account","details":[{"reason":"VALIDATION_REQUIRED","metadata":{"validation_url":"https://accounts.google.com/verify?t=1"}}]}}"#.to_vec(),
                });
            }
            Ok(win_http::HttpResponse { status: 200, body: b"{}".to_vec() })
        };
        assert!(s.m.refresh_quota_with(&id, &verify_http, &clients).is_err());
        let acc = s.m.state().unwrap().accounts.into_iter().next().unwrap();
        assert_eq!(acc.verification_url.as_deref(), Some("https://accounts.google.com/verify?t=1"));

        // 手动粘贴回调 URL 完成浏览器登录（使用不同邮箱以免与已导入账号合并）。
        let manual_given = std::sync::Mutex::new(Some(String::new()));
        let shown = RefCell::new(String::new());
        let http_manual = fake_google("manual@gmail.com", &calls);
        let manual_id = s.m.login_in_browser_with_full(
            &|url| {
                let query = url.split_once('?').unwrap().1;
                let state = query.split('&').find_map(|p| p.strip_prefix("state=")).unwrap();
                *manual_given.lock().unwrap() = Some(format!("http://127.0.0.1:9999/oauth-callback?code=the-code&state={}", state));
                *shown.borrow_mut() = url.to_string();
            },
            &|| false,
            &|| manual_given.lock().unwrap().take().filter(|s| !s.is_empty()),
            &http_manual,
            &clients,
            &|_| {},
        ).unwrap();
        assert_eq!(s.m.batch_delete(&[id, manual_id]).unwrap(), 2);
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
        let items = m.read_live_db().unwrap().expect("没有 Antigravity 登录");
        let (access, refresh, exp) = super::token_info(&items).expect("解析 oauthToken 失败");
        println!("live token: access {} chars, refresh {} chars, expiry {:?}", access.len(), refresh.len(), exp);
        println!("oauth clients found: {}", super::oauth_clients().len());
        println!("claude client id found: {}", super::super::claude_accounts::find_claude_client_id().is_some());
        assert!(!super::oauth_clients().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
