use crate::biz::pg_listener::PostgresDBListener;
use database_entity::AFCollabMember;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub enum CollabMemberAction {
  INSERT,
  UPDATE,
  DELETE,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CollabMemberChange {
  pub old: Option<AFCollabMember>,
  pub new: AFCollabMember,
  pub action_type: CollabMemberAction,
}

impl CollabMemberChange {
  pub fn uid(&self) -> i64 {
    self.new.uid
  }
  pub fn oid(&self) -> &str {
    &self.new.oid
  }
}

pub type CollabMemberListener = PostgresDBListener<CollabMemberChange>;
