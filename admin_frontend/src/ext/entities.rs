use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JsonResponse<T> {
  pub code: u16,
  pub data: T,
}

#[derive(Deserialize)]
pub struct UserUsageLimit {
  pub workspace_count: i64,
}

#[derive(Serialize)]
pub struct WorkspaceUsageLimits {
  pub name: String,
  pub member_count: usize,
  pub total_doc_size: String,
  pub total_blob_size: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct WorkspaceMember {
  pub name: String,
  pub email: String,
  pub role: String,
}

#[derive(Deserialize, Serialize)]
pub struct WorkspaceBlobUsage {
  pub consumed_capacity: u64,
}

#[derive(Deserialize)]
pub struct WorkspaceDocUsage {
  pub total_document_size: i64,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct UserProfile {
  pub uid: i64,
  pub uuid: Uuid,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub metadata: Option<serde_json::Value>,
  pub encryption_sign: Option<String>,
  pub latest_workspace_id: Uuid,
  pub updated_at: i64,
}
