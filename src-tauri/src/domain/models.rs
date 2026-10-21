use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::{MessageRole, MessageStatus, ProviderKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub default_model: String,
    pub enabled: bool,
    pub request_timeout_s: f64,
    pub max_input_tokens: Option<i64>,
    pub api_key_ciphertext: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub source_title: Option<String>,
    pub title_user_edited: bool,
    pub system_prompt: String,
    pub active_provider_id: Option<String>,
    pub active_model: Option<String>,
    pub active_leaf_message_id: Option<String>,
    pub project_path: Option<String>,
    pub source_kind: String,
    pub archived: bool,
    #[serde(default)]
    pub pinned_at: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub source_title: Option<String>,
    pub title_user_edited: bool,
    pub project_path: Option<String>,
    pub source_kind: String,
    pub archived: bool,
    #[serde(default)]
    pub pinned_at: Option<String>,
    pub message_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub id: String,
    pub conversation_id: String,
    pub parent_message_id: Option<String>,
    pub sequence_no: i64,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub remote_response_id: Option<String>,
    pub source_type: String,
    pub source_external_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCheckpoint {
    pub id: String,
    pub conversation_id: String,
    pub through_message_id: String,
    pub summary_version: i64,
    pub summary_text: String,
    pub created_by_provider: Option<String>,
    pub created_by_model: Option<String>,
    pub created_at: DateTime<Utc>,
}
