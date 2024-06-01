use serde::{Deserialize, Serialize, Serializer};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummarizeRowResponse {
  pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslateRowResponse {
  text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatQuestion {
  pub chat_id: String,
  pub data: MessageData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageData {
  pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatAnswer {
  pub content: String,
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
