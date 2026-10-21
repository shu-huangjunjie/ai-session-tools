use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedMessage {
    pub role: String,
    pub content: String,
    pub external_id: String,
    pub parent_external_id: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedConversation {
    pub title: String,
    pub external_id: String,
    pub messages: Vec<ImportedMessage>,
    pub active_external_id: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub source_type: String,
    pub source_path: String,
    pub source_sha256: String,
    pub conversation_count: usize,
    pub message_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<String>,
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub source_type: String,
    pub conversation_count: usize,
    pub new_conversation_count: usize,
    pub message_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<String>,
}
