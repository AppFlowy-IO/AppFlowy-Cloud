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
  pub status: ImportTaskStatus,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ImportTaskStatus {
  Pending,
  Completed,
  Failed,
}

impl From<i32> for ImportTaskStatus {
  fn from(status: i32) -> Self {
    match status {
      0 => ImportTaskStatus::Pending,
      1 => ImportTaskStatus::Completed,
      2 => ImportTaskStatus::Failed,
      _ => ImportTaskStatus::Pending,
    }
  }
}
