use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct JsonResponse<T> {
  pub code: u16,
  pub data: T,
}

#[derive(Deserialize)]
pub struct UserUsageLimit {
  pub workspace_count: Option<i64>,
}

#[derive(Serialize)]
pub struct WorkspaceUsage {
  pub name: String,
  pub member_count: i64,
  pub member_limit: i64,
  pub total_doc_size: String,
  pub total_blob_size: String,
  pub total_blob_limit: String,
}
