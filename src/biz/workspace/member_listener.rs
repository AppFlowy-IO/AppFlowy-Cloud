use crate::biz::pg_listener::PostgresDBListener;
use collab::preclude::Uuid;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub enum WorkspaceMemberAction {
  Insert,
  Update,
  Delete,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WorkspaceMemberChange {
  pub uid: i64,
  pub role_id: i64,
  pub workspace_id: Uuid,
  pub action_type: WorkspaceMemberAction,
}

pub type WorkspaceMemberListener = PostgresDBListener<WorkspaceMemberChange>;
