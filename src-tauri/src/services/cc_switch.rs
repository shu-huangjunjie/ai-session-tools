//! 只读地查看 cc-switch 管理了哪些技能和 MCP 服务器。
//!
//! cc-switch 在自己的数据库里保存技能和 MCP 的主副本，重新同步时会按数据库把
//! 客户端的技能目录和 MCP 配置改回去。界面据此提醒：这些条目在这里停用或删除后，
//! cc-switch 下次同步可能会恢复。这里只读数据库，从不写入。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

/// 一个条目在 cc-switch 中对 Claude、Codex 的启用状态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CcSwitchApps {
    pub claude: bool,
    pub codex: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CcSwitchState {
    /// 找到的数据库；没装 cc-switch 时为空。
    pub database: Option<PathBuf>,
    /// 键为小写的技能文件夹名。
    pub skills: HashMap<String, CcSwitchApps>,
    /// 键为 MCP 服务器名。
    pub mcp_servers: HashMap<String, CcSwitchApps>,
}

impl CcSwitchState {
    /// 按 cc-switch 的规则找数据目录：`%APPDATA%\com.ccswitch.desktop\app_paths.json`
    /// 里的 `app_config_dir_override`，没有则为 `~/.cc-switch`。读取失败时返回空状态。
    pub fn detect() -> Self {
        let override_dir = dirs::config_dir()
            .map(|d| d.join("com.ccswitch.desktop").join("app_paths.json"))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("app_config_dir_override")?.as_str().map(PathBuf::from));
        let dir = override_dir
            .filter(|d| d.is_dir())
            .or_else(|| dirs::home_dir().map(|h| h.join(".cc-switch")));
        dir.map(|d| Self::read(&d.join("cc-switch.db"))).unwrap_or_default()
    }

    pub fn read(database: &Path) -> Self {
        if !database.is_file() {
            return Self::default();
        }
        let Ok(conn) = Connection::open_with_flags(
            database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            return Self::default();
        };
        Self {
            database: Some(database.to_path_buf()),
            skills: query_apps(&conn, "SELECT directory, enabled_claude, enabled_codex FROM skills")
                .into_iter()
                .map(|(name, apps)| (name.to_lowercase(), apps))
                .collect(),
            mcp_servers: query_apps(&conn, "SELECT id, enabled_claude, enabled_codex FROM mcp_servers")
                .into_iter()
                .collect(),
        }
    }

    pub fn skill(&self, folder_name: &str) -> Option<CcSwitchApps> {
        self.skills.get(&folder_name.to_lowercase()).copied()
    }
}

/// 表或列不存在（cc-switch 版本不同）时返回空列表。
fn query_apps(conn: &Connection, sql: &str) -> Vec<(String, CcSwitchApps)> {
    let Ok(mut stmt) = conn.prepare(sql) else { return Vec::new() };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            CcSwitchApps {
                claude: row.get::<_, Option<bool>>(1)?.unwrap_or(false),
                codex: row.get::<_, Option<bool>>(2)?.unwrap_or(false),
            },
        ))
    });
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_skills_and_servers_and_tolerates_missing_tables() {
        let dir = std::env::temp_dir().join(format!("cv-ccswitch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cc-switch.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE skills (directory TEXT, enabled_claude BOOLEAN, enabled_codex BOOLEAN);
                 INSERT INTO skills VALUES ('Nature-Writing', 1, 0);
                 CREATE TABLE mcp_servers (id TEXT, enabled_claude BOOLEAN, enabled_codex BOOLEAN);
                 INSERT INTO mcp_servers VALUES ('pencil', 1, 1);",
            )
            .unwrap();
        }
        let state = CcSwitchState::read(&db);
        assert_eq!(state.skill("nature-writing"), Some(CcSwitchApps { claude: true, codex: false }));
        assert_eq!(state.mcp_servers.get("pencil"), Some(&CcSwitchApps { claude: true, codex: true }));

        let empty = dir.join("empty.db");
        Connection::open(&empty).unwrap();
        let state = CcSwitchState::read(&empty);
        assert!(state.database.is_some() && state.skills.is_empty() && state.mcp_servers.is_empty());
        assert!(CcSwitchState::read(&dir.join("missing.db")).database.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
