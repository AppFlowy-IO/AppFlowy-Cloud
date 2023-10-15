use crate::biz::pg_listener::PostgresDBListener;
use database_entity::AFRole;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub enum CollabMemberAction {
  Insert,
  Update,
  Delete,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CollabMemberChange {
  pub uid: i64,
  #[serde(rename = "role_id")]
  pub role: AFRole,
  pub oid: String,
  pub action_type: CollabMemberAction,
}

pub type CollabMemberListener = PostgresDBListener<CollabMemberChange>;
