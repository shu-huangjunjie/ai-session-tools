use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenaiResponses,
    OpenaiCompatibleChat,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenaiResponses => "openai_responses",
            ProviderKind::OpenaiCompatibleChat => "openai_compatible_chat",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "openai_responses" => ProviderKind::OpenaiResponses,
            _ => ProviderKind::OpenaiCompatibleChat,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    Developer,
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::Developer => "developer",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "system" | "系统" => MessageRole::System,
            "developer" => MessageRole::Developer,
            "assistant" | "助手" => MessageRole::Assistant,
            _ => MessageRole::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Complete,
    Streaming,
    Interrupted,
    Failed,
    Cancelled,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageStatus::Complete => "complete",
            MessageStatus::Streaming => "streaming",
            MessageStatus::Interrupted => "interrupted",
            MessageStatus::Failed => "failed",
            MessageStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "streaming" => MessageStatus::Streaming,
            "interrupted" => MessageStatus::Interrupted,
            "failed" => MessageStatus::Failed,
            "cancelled" => MessageStatus::Cancelled,
            _ => MessageStatus::Complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttemptStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
