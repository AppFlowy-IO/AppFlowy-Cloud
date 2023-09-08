use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollabParams {
  pub object_id: String,
  pub raw_data: Vec<u8>,
  pub len: usize,
  pub workspace_id: String,
}

impl CreateCollabParams {
  pub fn from_raw_data(object_id: &str, raw_data: Vec<u8>, workspace_id: &str) -> Self {
    let len = raw_data.len();
    Self {
      object_id: object_id.to_string(),
      raw_data,
      len,
      workspace_id: workspace_id.to_string(),
    }
  }
}
