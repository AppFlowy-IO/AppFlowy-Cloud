use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub const STREAM_METADATA_KEY: &str = "0";
pub const STREAM_ANSWER_KEY: &str = "1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummarizeRowResponse {
  pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatQuestionQuery {
  pub chat_id: String,
  pub question_id: i64,
  pub format: ResponseFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatQuestion {
  pub chat_id: String,
  pub data: MessageData,
  #[serde(default)]
  pub format: ResponseFormat,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ResponseFormat {
  #[serde(rename = "type")]
  pub format_type: FormatType,
  pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum FormatType {
  #[default]
  Paragraph = 0,
  BulletList = 1,
  NumberedList = 2,
  SimpleTable = 3,
  // Image = 4,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageData {
  pub content: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub metadata: Option<serde_json::Value>,
  #[serde(default)]
  pub rag_ids: Vec<String>,
  #[serde(default)]
  pub message_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatAnswer {
  pub content: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepeatedRelatedQuestion {
  pub message_id: i64,
  pub items: Vec<RelatedQuestion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelatedQuestion {
  pub content: String,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompleteTextResponse {
  pub text: String,
}

#[derive(Clone, Debug, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum CompletionType {
  ImproveWriting = 1,
  SpellingAndGrammar = 2,
  MakeShorter = 3,
  MakeLonger = 4,
  ContinueWriting = 5,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchDocumentsRequest {
  #[serde(serialize_with = "serialize_workspaces")]
  pub workspaces: Vec<String>,
  pub query: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub result_count: Option<u32>,
}

#[allow(clippy::ptr_arg)]
fn serialize_workspaces<S>(workspaces: &Vec<String>, serializer: S) -> Result<S::Ok, S::Error>
where
  S: Serializer,
{
  let workspaces = workspaces.join(",");
  serializer.serialize_str(&workspaces)
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Document {
  pub id: String,
  #[serde(rename = "type")]
  pub doc_type: CollabType,
  pub workspace_id: String,
  pub content: String,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize_repr, Deserialize_repr)]
pub enum CollabType {
  Document = 0,
  Database = 1,
  WorkspaceDatabase = 2,
  Folder = 3,
  DatabaseRow = 4,
  UserAwareness = 5,
  Unknown = 6,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslateRowParams {
  pub workspace_id: String,
  pub data: TranslateRowData,
}

/// Represents different types of content that can be used to summarize a database row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslateRowData {
  pub cells: Vec<TranslateItem>,
  pub language: String,
  pub include_header: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslateItem {
  pub title: String,
  pub content: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TranslateRowResponse {
  pub items: Vec<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum EmbeddingInput {
  /// The string that will be turned into an embedding.
  String(String),
  /// The array of strings that will be turned into an embedding.
  StringArray(Vec<String>),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum EmbeddingOutput {
  Float(Vec<f64>),
  Base64(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Embedding {
  /// An integer representing the index of the embedding in the list of embeddings.
  pub index: i32,
  /// The embedding value, which is an instance of `EmbeddingOutput`.
  pub embedding: EmbeddingOutput,
}

/// https://platform.openai.com/docs/api-reference/embeddings
#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAIEmbeddingResponse {
  /// A string that is always set to "embedding".
  pub object: String,
  /// A list of `Embedding` objects.
  pub data: Vec<Embedding>,
  /// A string representing the model used to generate the embeddings.
  pub model: String,
  /// An integer representing the total number of tokens used.
  pub usage: EmbeddingUsage,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbeddingUsage {
  #[serde(default)]
  pub prompt_tokens: i32,
  pub total_tokens: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingEncodingFormat {
  Float,
  Base64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmbeddingRequest {
  /// An instance of `EmbeddingInput` containing the data to be embedded.
  pub input: EmbeddingInput,
  /// A string representing the model to use for generating embeddings.
  pub model: String,
  /// An instance of `EmbeddingEncodingFormat` representing the format of the embedding.
  pub encoding_format: EmbeddingEncodingFormat,
  /// An integer representing the number of dimensions for the embedding.
  pub dimensions: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmbeddingModel {
  #[serde(rename = "text-embedding-3-small")]
  TextEmbedding3Small,
  #[serde(rename = "text-embedding-3-large")]
  TextEmbedding3Large,
  #[serde(rename = "text-embedding-ada-002")]
  TextEmbeddingAda002,
}

impl EmbeddingModel {
  pub fn supported_models() -> &'static [&'static str] {
    &[
      "text-embedding-ada-002",
      "text-embedding-3-small",
      "text-embedding-3-large",
    ]
  }

  pub fn max_token(&self) -> usize {
    match self {
      EmbeddingModel::TextEmbeddingAda002 => 8191,
      EmbeddingModel::TextEmbedding3Large => 8191,
      EmbeddingModel::TextEmbedding3Small => 8191,
    }
  }

  pub fn default_dimensions(&self) -> i32 {
    match self {
      EmbeddingModel::TextEmbeddingAda002 => 1536,
      EmbeddingModel::TextEmbedding3Large => 3072,
      EmbeddingModel::TextEmbedding3Small => 1536,
    }
  }

  pub fn name(&self) -> &'static str {
    match self {
      EmbeddingModel::TextEmbeddingAda002 => "text-embedding-ada-002",
      EmbeddingModel::TextEmbedding3Large => "text-embedding-3-large",
      EmbeddingModel::TextEmbedding3Small => "text-embedding-3-small",
    }
  }

  pub fn from_name(name: &str) -> Option<Self> {
    match name {
      "text-embedding-ada-002" => Some(EmbeddingModel::TextEmbeddingAda002),
      "text-embedding-3-large" => Some(EmbeddingModel::TextEmbedding3Large),
      "text-embedding-3-small" => Some(EmbeddingModel::TextEmbedding3Small),
      _ => None,
    }
  }
}

impl Display for EmbeddingModel {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      EmbeddingModel::TextEmbedding3Small => write!(f, "text-embedding-3-small"),
      EmbeddingModel::TextEmbedding3Large => write!(f, "text-embedding-3-large"),
      EmbeddingModel::TextEmbeddingAda002 => write!(f, "text-embedding-ada-002"),
    }
  }
}

#[derive(Debug, Clone, Default, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum AIModel {
  #[default]
  DefaultModel = 0,
  GPT4oMini = 1,
  GPT4o = 2,
  Claude3Sonnet = 3,
  Claude3Opus = 4,
}

impl AIModel {
  pub fn to_str(&self) -> &str {
    match self {
      AIModel::DefaultModel => "default-model",
      AIModel::GPT4oMini => "gpt-4o-mini",
      AIModel::GPT4o => "gpt-4o",
      AIModel::Claude3Sonnet => "claude-3-sonnet",
      AIModel::Claude3Opus => "claude-3-opus",
    }
  }
}

impl FromStr for AIModel {
  type Err = anyhow::Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "gpt-3.5-turbo" => Ok(AIModel::GPT4oMini),
      "gpt-4o-mini" => Ok(AIModel::GPT4oMini),
      "gpt-4o" => Ok(AIModel::GPT4o),
      "claude-3-sonnet" => Ok(AIModel::Claude3Sonnet),
      "claude-3-opus" => Ok(AIModel::Claude3Opus),
      _ => Ok(AIModel::DefaultModel),
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepeatedLocalAIPackage(pub Vec<AppFlowyOfflineAI>);

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AppFlowyOfflineAI {
  pub app_name: String,
  pub ai_plugin_name: String,
  pub version: String,
  pub url: String,
  pub etag: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct LLMModel {
  pub llm_id: i64,
  pub provider: String,
  pub embedding_model: ModelInfo,
  pub chat_model: ModelInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ModelInfo {
  pub name: String,
  pub file_name: String,
  pub file_size: i64,
  pub requirements: String,
  pub download_url: String,
  pub desc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalAIConfig {
  pub models: Vec<LLMModel>,
  pub plugin: AppFlowyOfflineAI,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateChatContext {
  pub chat_id: String,
  pub context_loader: String,
  pub content: String,
  pub chunk_size: i32,
  pub chunk_overlap: i32,
  pub metadata: serde_json::Value,
}

impl CreateChatContext {
  pub fn new(chat_id: String, context_loader: String, text: String) -> Self {
    CreateChatContext {
      chat_id,
      context_loader,
      content: text,
      chunk_size: 2000,
      chunk_overlap: 20,
      metadata: json!({}),
    }
  }

  pub fn with_metadata<T: Serialize>(mut self, metadata: T) -> Self {
    self.metadata = json!(metadata);
    self
  }
}

impl Display for CreateChatContext {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "Create Chat context: {{ chat_id: {}, content_type: {}, content size: {},  metadata: {:?} }}",
      self.chat_id,
      self.context_loader,
      self.content.len(),
      self.metadata
    ))
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomPrompt {
  pub system: String,
  pub user: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalculateSimilarityParams {
  pub workspace_id: String,
  pub input: String,
  pub expected: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimilarityResponse {
  pub score: f64,
}
