use serde::Deserialize;

#[derive(Deserialize)]
pub struct GetCollabHistoryRequest {
  object_id: String,
}

pub enum CollabHistoryMessage {
  HistoryUpdate { oid: String, updates: Vec<u8> },
}
