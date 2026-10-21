use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::MessageRole;
use crate::persistence::{Database, Repository};
use crate::util::sanitize_file_name;

pub struct DataService<'a> {
    db: &'a Database,
}

impl<'a> DataService<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn backup(&self) -> Result<PathBuf, String> {
        self.db.backup(None).map_err(|e| format!("备份失败：{}", e))
    }

    pub fn export_json(&self, conversation_id: &str, destination: &Path) -> Result<(), String> {
        let repo = Repository::new(self.db);
        let data = repo
            .export_conversation(conversation_id)
            .map_err(|e| format!("导出数据读取失败：{}", e))?;

        let json_str = serde_json::to_string_pretty(&data)
            .map_err(|e| format!("序列化 JSON 失败：{}", e))?;

        fs::write(destination, json_str).map_err(|e| format!("写入文件失败：{}", e))?;
        Ok(())
    }

    pub fn export_markdown(&self, conversation_id: &str, destination: &Path) -> Result<(), String> {
        let repo = Repository::new(self.db);
        let conv = repo
            .get_conversation(conversation_id)
            .map_err(|e| format!("读取会话失败：{}", e))?
            .ok_or_else(|| "会话不存在".to_string())?;

        let msgs = repo
            .load_active_path(conversation_id)
            .map_err(|e| format!("读取消息失败：{}", e))?;

        let mut lines = Vec::new();
        lines.push(format!("# {}", conv.title));
        lines.push(String::new());
        lines.push(format!(
            "> 创建时间：{}",
            conv.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S")
        ));
        lines.push(format!(
            "> 更新时间：{}",
            conv.updated_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S")
        ));
        if let Some(ref proj) = conv.project_path {
            lines.push(format!("> 项目目录：{}", proj));
        }
        lines.push(String::new());

        if !conv.system_prompt.is_empty() {
            lines.push("## System".to_string());
            lines.push(String::new());
            lines.push(conv.system_prompt.clone());
            lines.push(String::new());
        }

        for msg in msgs {
            let role_name = match msg.role {
                MessageRole::System => "System",
                MessageRole::Developer => "Developer",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
            };
            lines.push(format!("## {}", role_name));
            lines.push(String::new());
            lines.push(msg.content);
            lines.push(String::new());
        }

        fs::write(destination, lines.join("\n")).map_err(|e| format!("写入 Markdown 失败：{}", e))?;
        Ok(())
    }

    pub fn batch_export_markdown(
        &self,
        conversation_ids: &[String],
        target_dir: &Path,
    ) -> Result<usize, String> {
        if !target_dir.exists() {
            fs::create_dir_all(target_dir).map_err(|e| format!("创建目标目录失败：{}", e))?;
        }

        let repo = Repository::new(self.db);
        let mut count = 0;
        let mut used_filenames = std::collections::HashSet::new();
        let mut failures = Vec::new();

        for cid in conversation_ids {
            let conv = match repo.get_conversation(cid) {
                Ok(Some(c)) => c,
                _ => continue,
            };

            let base_name = sanitize_file_name(&conv.title, "conversation");

            let mut final_name = format!("{}.md", base_name);
            let mut suffix = 1;
            while used_filenames.contains(&final_name.to_lowercase())
                || target_dir.join(&final_name).exists()
            {
                final_name = format!("{} ({}).md", base_name, suffix);
                suffix += 1;
            }
            used_filenames.insert(final_name.to_lowercase());

            let dest = target_dir.join(&final_name);
            match self.export_markdown(cid, &dest) {
                Ok(()) => count += 1,
                Err(e) => failures.push(format!("{}：{}", conv.title, e)),
            }
        }

        if count == 0 && !failures.is_empty() {
            return Err(format!("导出失败：{}", failures.join("；")));
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::database::Database;
    use std::sync::Arc;

    #[test]
    fn test_batch_export_markdown() {
        let temp_dir = std::env::temp_dir().join(format!("cv_batch_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let backups = temp_dir.join("backups");

        let db = Arc::new(Database::new(&db_path, &backups).unwrap());
        let service = DataService::new(&db);

        {
            let conn = db.lock_conn();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO conversations (id, title, source_title, title_user_edited, system_prompt, project_path, source_kind, archived, created_at, updated_at)
                 VALUES ('test-c1', '会话测试 1', '会话测试 1', 0, '系统提示词', 'C:\\proj1', 'antigravity', 0, ?1, ?2)",
                rusqlite::params![now, now],
            ).unwrap();
            conn.execute(
                "INSERT INTO conversations (id, title, source_title, title_user_edited, system_prompt, project_path, source_kind, archived, created_at, updated_at)
                 VALUES ('test-c2', '会话测试 2', '会话测试 2', 0, '', NULL, 'antigravity', 0, ?1, ?2)",
                rusqlite::params![now, now],
            ).unwrap();
        }

        let export_dir = temp_dir.join("exports");
        let count = service.batch_export_markdown(&["test-c1".to_string(), "test-c2".to_string()], &export_dir).unwrap();
        assert_eq!(count, 2);
        assert!(export_dir.join("会话测试 1.md").exists());
        assert!(export_dir.join("会话测试 2.md").exists());

        let _ = fs::remove_dir_all(temp_dir);
    }
}
