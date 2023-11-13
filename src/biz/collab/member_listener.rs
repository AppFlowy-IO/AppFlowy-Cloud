use crate::biz::pg_listener::PostgresDBListener;
use database_entity::pg_row::AFCollabMemberRow;
use serde::Deserialize;

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Clone, Debug)]
pub enum CollabMemberAction {
  INSERT,
  UPDATE,
  DELETE,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CollabMemberNotification {
  /// The old will be None if the row does not exist before
  pub old: Option<AFCollabMemberRow>,
  /// The new will be None if the row is deleted
  pub new: Option<AFCollabMemberRow>,
  /// Represent the action of the database. Such as INSERT, UPDATE, DELETE
  pub action_type: CollabMemberAction,
}

impl CollabMemberNotification {
  pub fn old_uid(&self) -> Option<&i64> {
    self.old.as_ref().map(|o| &o.uid)
  }

  pub fn old_oid(&self) -> Option<&str> {
    self.old.as_ref().map(|o| o.oid.as_str())
  }
  pub fn new_uid(&self) -> Option<&i64> {
    self.new.as_ref().map(|n| &n.uid)
  }
  pub fn new_oid(&self) -> Option<&str> {
    self.new.as_ref().map(|n| n.oid.as_str())
  }
}

pub type CollabMemberListener = PostgresDBListener<CollabMemberNotification>;
