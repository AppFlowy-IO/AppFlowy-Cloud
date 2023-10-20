use crate::biz::pg_listener::PostgresDBListener;
use serde::Deserialize;
use uuid::Uuid;

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Clone, Debug)]
pub enum WorkspaceMemberAction {
  INSERT,
  UPDATE,
  DELETE,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WorkspaceMemberChange {
  pub old: Option<WorkspaceMemberRow>,
  pub new: Option<WorkspaceMemberRow>,
  pub action_type: WorkspaceMemberAction,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WorkspaceMemberRow {
  pub uid: i64,
  pub role_id: i64,
  pub workspace_id: Uuid,
}

pub type WorkspaceMemberListener = PostgresDBListener<WorkspaceMemberChange>;
