use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct SearchDocumentParams {
  pub query: String,
  pub workspace_id: String,
  pub response_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchDocumentResponse {
  pub data: Vec<SearchDocumentResponseItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchDocumentResponseItem {
  pub object_id: String,
  pub workspace_id: String,
  pub score: f64,
  pub preview: String,
  pub created_by: String,
  pub created_at: DateTime<Utc>,
  pub modified_at: DateTime<Utc>,
}
