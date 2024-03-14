use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct JsonResponse<T> {
  pub code: u16,
  pub data: T,
}

#[derive(Deserialize)]
pub struct UserUsageLimit {
  pub workspace_count: i64,
}

#[derive(Serialize)]
pub struct WorkspaceUsage {
  pub name: String,
  pub member_count: usize,
  pub member_limit: String,
  pub total_doc_size: String,
  pub total_blob_size: String,
  pub total_blob_limit: String,
}

#[derive(Deserialize)]
pub struct WorkspaceMember {
  pub name: String,
  pub email: String,
  pub role: String,
}

#[derive(Deserialize)]
pub struct WorkspaceUsageLimit {
  pub total_blob_size: i64,
  pub single_blob_size: i64,
  pub member_count: i64,
}

#[derive(Deserialize, Serialize)]
pub struct WorkspaceBlobUsage {
  pub consumed_capacity: u64,
}
