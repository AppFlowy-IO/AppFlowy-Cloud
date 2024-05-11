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
  pub offset: u64,
  pub limit: u64,
  pub after_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
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
