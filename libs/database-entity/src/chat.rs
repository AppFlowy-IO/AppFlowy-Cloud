use crate::util::validate_not_empty_str;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatParams {
  #[validate(custom = "validate_not_empty_str")]
  pub chat_id: String,

  #[validate(custom = "validate_not_empty_str")]
  pub name: String,

  pub rag_ids: Vec<String>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct UpdateChatParams {
  #[validate(custom = "validate_not_empty_str")]
  pub chat_id: String,

  #[validate(custom = "validate_not_empty_str")]
  pub name: Option<String>,

  pub rag_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatMessageParams {
  #[validate(custom = "validate_not_empty_str")]
  pub content: String,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct GetChatMessageParams {
  #[validate(custom = "validate_not_empty_str")]
  pub chat_id: String,
  pub offset: MessageOffset,
  pub limit: u64,
}

impl GetChatMessageParams {
  pub fn offset(chat_id: String, offset: u64, limit: u64) -> Self {
    Self {
      chat_id,
      offset: MessageOffset::Offset(offset),
      limit,
    }
  }

  pub fn after_message_id(chat_id: String, after_message_id: i64, limit: u64) -> Self {
    Self {
      chat_id,
      offset: MessageOffset::AfterMessageId(after_message_id),
      limit,
    }
  }
  pub fn before_message_id(chat_id: String, before_message_id: i64, limit: u64) -> Self {
    Self {
      chat_id,
      offset: MessageOffset::BeforeMessageId(before_message_id),
      limit,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageOffset {
  Offset(u64),
  AfterMessageId(i64),
  BeforeMessageId(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
  pub author: ChatAuthor,
  pub message_id: i64,
  pub content: String,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedChatMessage {
  pub messages: Vec<ChatMessage>,
  pub has_more: bool,
  pub total: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum ChatAuthor {
  #[default]
  Unknown,
  Human {
    uid: i64,
  },
  System,
  AI,
}

impl From<serde_json::Value> for ChatAuthor {
  fn from(value: serde_json::Value) -> Self {
    serde_json::from_value::<ChatAuthor>(value).unwrap_or_default()
  }
}
