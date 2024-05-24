use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct SearchDocumentRequest {
  pub query: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub preview_size: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchDocumentResponse {
  pub data: Vec<SearchDocumentResponseItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchDocumentResponseItem {
  pub object_id: String,
  pub workspace_id: String,
  pub score: f32,
  pub preview: String,
  pub created_by: String,
  pub created_at: DateTime<Utc>,
}
