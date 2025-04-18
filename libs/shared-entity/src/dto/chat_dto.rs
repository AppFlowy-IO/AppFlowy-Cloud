use chrono::{DateTime, Utc};
use infra::validate::validate_not_empty_str;
use serde::{Deserialize, Serialize};

use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;
use std::fmt::Display;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatParams {
  #[validate(custom(function = "validate_not_empty_str"))]
  pub chat_id: String,
  pub name: String,
  pub rag_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct UpdateChatParams {
  #[validate(custom(function = "validate_not_empty_str"))]
  pub name: Option<String>,

  /// Key-value pairs of metadata to be updated.
  pub metadata: Option<serde_json::Value>,

  pub rag_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatMessageParams {
  #[validate(custom(function = "validate_not_empty_str"))]
  pub content: String,
  pub message_type: ChatMessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMetadataDescription {
  pub id: String,
  pub name: String,
  pub source: String,
  pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRAGData {
  /// The textual content of the metadata. This field can contain raw text data from a specific
  /// document or any other text content that is indexable. This content is typically used for
  /// search and indexing purposes within the chat context.
  pub content: String,

  /// The type of content represented by this metadata. This could indicate the format or
  /// nature of the content (e.g., text, markdown, PDF). The `content_type` helps in
  /// processing or rendering the content appropriately.
  pub content_type: ContextLoader,

  /// The size of the content in bytes.
  pub size: i64,
}

impl ChatRAGData {
  pub fn from_text(text: String) -> Self {
    let size = text.len() as i64;
    Self {
      content: text,
      content_type: ContextLoader::Text,
      size,
    }
  }
}

impl ChatRAGData {
  /// Validates the `ChatMetadataData` instance.
  ///
  /// This method checks the validity of the data based on the content type and the presence of content or URL.
  /// - If `content` is empty, the method checks if `url` is provided. If `url` is also empty, the data is invalid.
  /// - For `Text` and `Markdown`, it ensures that the content length matches the specified size if content is present.
  /// - For `Unknown` and `PDF`, it currently returns `false` as these types are either unsupported or
  ///   require additional validation logic.
  ///
  /// Returns `true` if the data is valid according to its content type and the presence of content or URL, otherwise `false`.
  pub fn validate(&self) -> Result<(), anyhow::Error> {
    match self.content_type {
      ContextLoader::Text | ContextLoader::Markdown => {
        if self.content.len() != self.size as usize {
          return Err(anyhow::anyhow!(
            "Invalid content size: content size: {}, expected size: {}",
            self.content.len(),
            self.size
          ));
        }
      },
      ContextLoader::PDF => {
        if self.content.is_empty() {
          return Err(anyhow::anyhow!("Invalid content: content is empty"));
        }
      },
      ContextLoader::Unknown => {
        return Err(anyhow::anyhow!(
          "Unsupported content type: {:?}",
          self.content_type
        ));
      },
    }
    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextLoader {
  Unknown,
  Text,
  Markdown,
  PDF,
}

impl Display for ContextLoader {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ContextLoader::Unknown => write!(f, "unknown"),
      ContextLoader::Text => write!(f, "text"),
      ContextLoader::Markdown => write!(f, "markdown"),
      ContextLoader::PDF => write!(f, "pdf"),
    }
  }
}

impl ChatRAGData {
  pub fn new_text(content: String) -> Self {
    let size = content.len();
    Self {
      content,
      content_type: ContextLoader::Text,
      size: size as i64,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageMetaParams {
  pub message_id: i64,
  pub meta_data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageContentParams {
  pub chat_id: String,
  pub message_id: i64,
  pub content: String,
  #[serde(default)]
  pub model: String,
}

#[derive(Debug, Clone, Default, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChatMessageType {
  System = 0,
  #[default]
  User = 1,
}

impl CreateChatMessageParams {
  pub fn new_system<T: ToString>(content: T) -> Self {
    Self {
      content: content.to_string(),
      message_type: ChatMessageType::System,
    }
  }

  pub fn new_user<T: ToString>(content: T) -> Self {
    Self {
      content: content.to_string(),
      message_type: ChatMessageType::User,
    }
  }
}
#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct GetChatMessageParams {
  pub cursor: MessageCursor,
  pub limit: u64,
}

impl GetChatMessageParams {
  pub fn offset(offset: u64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::Offset(offset),
      limit,
    }
  }

  pub fn after_message_id(after_message_id: i64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::AfterMessageId(after_message_id),
      limit,
    }
  }
  pub fn before_message_id(before_message_id: i64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::BeforeMessageId(before_message_id),
      limit,
    }
  }

  pub fn next_back(limit: u64) -> Self {
    Self {
      cursor: MessageCursor::NextBack,
      limit,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageCursor {
  Offset(u64),
  AfterMessageId(i64),
  BeforeMessageId(i64),
  NextBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
  pub author: ChatAuthor,
  pub message_id: i64,
  pub content: String,
  pub created_at: DateTime<Utc>,
  #[serde(rename = "meta_data")]
  pub metadata: serde_json::Value,
  /// When current message is a question, then reply_message_id is None
  /// When current message is an answer, then reply_message_id is the question message id
  pub reply_message_id: Option<i64>,
}

impl ChatMessage {
  pub fn new_human(message_id: i64, content: String, reply_message_id: Option<i64>) -> Self {
    Self {
      author: ChatAuthor::new(message_id, ChatAuthorType::Human),
      message_id,
      content,
      created_at: Utc::now(),
      metadata: json!({}),
      reply_message_id,
    }
  }

  pub fn new_ai(message_id: i64, content: String, reply_message_id: Option<i64>) -> Self {
    Self {
      author: ChatAuthor::ai(),
      message_id,
      content,
      created_at: Utc::now(),
      metadata: json!({}),
      reply_message_id,
    }
  }

  pub fn new_system(message_id: i64, content: String) -> Self {
    Self {
      author: ChatAuthor::new(message_id, ChatAuthorType::System),
      message_id,
      content,
      created_at: Utc::now(),
      metadata: json!({}),
      reply_message_id: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageWithAuthorUuid {
  pub author: ChatAuthorWithUuid,
  pub message_id: i64,
  pub content: String,
  #[serde(rename = "meta_data")]
  pub metadata: serde_json::Value,
  pub created_at: DateTime<Utc>,
  pub reply_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QAChatMessage {
  pub question: ChatMessage,
  pub answer: Option<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedChatMessage {
  pub messages: Vec<ChatMessage>,
  pub has_more: bool,
  pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedChatMessageWithAuthorUuid {
  pub messages: Vec<ChatMessageWithAuthorUuid>,
  pub has_more: bool,
  pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSettings {
  // Currently we have not used the `name` field in the ChatSettings
  pub name: String,
  pub rag_ids: Vec<String>,
  pub metadata: serde_json::Value,
}

#[derive(Debug, Default, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChatAuthorType {
  Unknown = 0,
  Human = 1,
  #[default]
  System = 2,
  AI = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAuthor {
  pub author_id: i64,
  #[serde(default)]
  pub author_type: ChatAuthorType,
  #[serde(default)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub meta: Option<serde_json::Value>,
}

impl ChatAuthor {
  pub fn new(author_id: i64, author_type: ChatAuthorType) -> Self {
    Self {
      author_id,
      author_type,
      meta: None,
    }
  }

  pub fn ai() -> Self {
    Self {
      author_id: 0,
      author_type: ChatAuthorType::AI,
      meta: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAuthorWithUuid {
  pub author_id: i64,
  pub author_uuid: Uuid,
  #[serde(default)]
  pub author_type: ChatAuthorType,
  #[serde(default)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub meta: Option<serde_json::Value>,
}

impl ChatAuthorWithUuid {
  pub fn new(author_id: i64, author_uuid: Uuid, author_type: ChatAuthorType) -> Self {
    Self {
      author_id,
      author_uuid,
      author_type,
      meta: None,
    }
  }

  pub fn ai() -> Self {
    Self {
      author_id: 0,
      author_uuid: Uuid::nil(),
      author_type: ChatAuthorType::AI,
      meta: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageResponse {
  pub answer: Option<ChatMessage>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateAnswerMessageParams {
  #[validate(custom(function = "validate_not_empty_str"))]
  pub content: String,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub metadata: Option<serde_json::Value>,

  pub question_message_id: i64,
}
