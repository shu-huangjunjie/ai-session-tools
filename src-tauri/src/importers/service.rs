use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::models::{ImportPreview, ImportResult};
use super::parsers::{parse_file, probe_format};
use crate::persistence::{Database, ImportedMessagePayload, Repository};

pub struct ImportService<'a> {
    db: &'a Database,
}

impl<'a> ImportService<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn source_hash(path: &Path) -> Result<String, String> {
        let mut file = File::open(path).map_err(|e| format!("打开文件计算哈希失败: {}", e))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub fn preview(&self, path: &Path) -> Result<ImportPreview, String> {
        let source_type = probe_format(path)?;
        let sha256 = Self::source_hash(path)?;
        let conversations = parse_file(path, &source_type)?;

        let mut message_count = 0;
        let mut warnings = Vec::new();
        let mut sample_titles = Vec::new();

        for (idx, c) in conversations.iter().enumerate() {
            message_count += c.messages.len();
            warnings.extend(c.warnings.clone());
            if idx < 5 {
                sample_titles.push(c.title.clone());
            }
        }

        Ok(ImportPreview {
            source_type,
            source_path: path.to_string_lossy().to_string(),
            source_sha256: sha256,
            conversation_count: conversations.len(),
            message_count,
            warning_count: warnings.len(),
            warnings,
            sample_titles,
        })
    }

    pub fn import_file(&self, path: &Path) -> Result<ImportResult, String> {
        let source_type = probe_format(path)?;
        let conversations = parse_file(path, &source_type)?;
        let repo = Repository::new(self.db);

        let mut conv_count = 0;
        let mut new_conv_count = 0;
        let mut msg_count = 0;
        let mut warnings = Vec::new();

        for c in &conversations {
            warnings.extend(c.warnings.clone());

            let msg_payloads: Vec<ImportedMessagePayload> = c.messages.iter().map(|m| {
                ImportedMessagePayload {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    external_id: m.external_id.clone(),
                    parent_external_id: m.parent_external_id.clone(),
                    created_at: m.created_at.clone(),
                }
            }).collect();

            match repo.sync_import_conversation(
                &c.title,
                &source_type,
                &c.external_id,
                &msg_payloads,
                c.active_external_id.as_deref(),
                None,
                "general",
            ) {
                Ok((_id, inserted, created, _)) => {
                    conv_count += 1;
                    if created {
                        new_conv_count += 1;
                    }
                    msg_count += inserted;
                }
                Err(e) => {
                    warnings.push(format!("导入“{}”失败: {}", c.title, e));
                }
            }
        }

        Ok(ImportResult {
            source_type,
            conversation_count: conv_count,
            new_conversation_count: new_conv_count,
            message_count: msg_count,
            warning_count: warnings.len(),
            warnings,
        })
    }
}
