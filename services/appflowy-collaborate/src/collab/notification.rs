use access_control::access::{AccessControl, ObjectType};
use access_control::act::ActionVariant;
use database::pg_row::AFCollabMemberRow;
use database::workspace::select_permission;
use serde::Deserialize;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::error;
use tracing::log::warn;

#[allow(dead_code)]
pub fn spawn_listen_on_collab_member_change(
  pg_pool: PgPool,
  mut listener: broadcast::Receiver<CollabMemberNotification>,
  access_control: AccessControl,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
          if let Some(member_row) = change.new {
            let permission_row = select_permission(&pg_pool, &member_row.permission_id).await;
            if let Ok(Some(row)) = permission_row {
              if let Err(err) = access_control
                .update_policy(
                  &member_row.uid,
                  ObjectType::Collab(&member_row.oid),
                  ActionVariant::FromAccessLevel(&row.access_level),
                )
                .await
              {
                error!(
                  "Failed to update the user:{} collab{} access control, error: {}",
                  member_row.uid, member_row.oid, err
                );
              }
            }
          } else {
            error!("The new collab member is None")
          }
        },
        CollabMemberAction::DELETE => {
          if let (Some(oid), Some(uid)) = (change.old_oid(), change.old_uid()) {
            if let Err(err) = access_control
              .remove_policy(uid, &ObjectType::Collab(oid))
              .await
            {
              warn!(
                "Failed to remove the user:{} collab{} access control, error: {}",
                uid, oid, err
              );
            }
          } else {
            warn!("The oid or uid is None")
          }
        },
      }
    }
  });
}

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
