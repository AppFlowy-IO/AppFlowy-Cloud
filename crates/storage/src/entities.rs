use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollab {
  pub object_id: String,
  pub raw_data: Vec<u8>,
  pub len: usize,
  pub workspace_id: String,
  pub encrypt: i32,
}
