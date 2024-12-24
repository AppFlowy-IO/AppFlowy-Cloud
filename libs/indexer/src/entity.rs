use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use database_entity::dto::AFCollabEmbeddedChunk;
use uuid::Uuid;

pub struct UnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}

pub struct EmbeddingRecord {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub tokens_used: u32,
  pub contents: Vec<AFCollabEmbeddedChunk>,
}

impl EmbeddingRecord {
  pub fn empty(workspace_id: Uuid, object_id: String, collab_type: CollabType) -> Self {
    Self {
      workspace_id,
      object_id,
      collab_type,
      tokens_used: 0,
      contents: vec![],
    }
  }
}
