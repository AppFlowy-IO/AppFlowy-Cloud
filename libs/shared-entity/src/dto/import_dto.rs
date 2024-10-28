use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UserImportTask {
  pub tasks: Vec<ImportTaskDetail>,
  pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportTaskDetail {
  pub task_id: String,
  pub file_size: u64,
  pub created_at: i64,
  pub status: i16,
}
