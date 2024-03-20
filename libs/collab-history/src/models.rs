use serde::Deserialize;

#[derive(Deserialize)]
pub struct GetCollabHistoryRequest {
  workspace_id: String,
  object_id: String,
}

pub enum CollabHistoryMessage {
  HistoryUpdate { oid: String, updates: Vec<u8> },
}
