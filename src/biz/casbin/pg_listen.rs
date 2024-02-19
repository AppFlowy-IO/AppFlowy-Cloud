use crate::biz::casbin::access_control::{ActionType, ObjectType};
use crate::biz::casbin::enforcer::AFEnforcer;
use crate::biz::pg_listener::PostgresDBListener;
use database::pg_row::AFCollabMemberRow;
use database::workspace::select_permission;
use database_entity::dto::AFRole;
use serde::Deserialize;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::error;
use tracing::log::warn;
use uuid::Uuid;

pub(crate) fn spawn_listen_on_collab_member_change(
  pg_pool: PgPool,
  mut listener: broadcast::Receiver<CollabMemberNotification>,
  enforcer: Arc<AFEnforcer>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
          if let Some(member_row) = change.new {
            if let Ok(Some(row)) = select_permission(&pg_pool, &member_row.permission_id).await {
              if let Err(err) = enforcer
                .update(
                  &member_row.uid,
                  &ObjectType::Collab(&member_row.oid),
                  &ActionType::Level(row.access_level),
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
            if let Err(err) = enforcer.remove(uid, &ObjectType::Collab(oid)).await {
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

pub(crate) fn spawn_listen_on_workspace_member_change(
  mut listener: broadcast::Receiver<WorkspaceMemberNotification>,
  enforcer: Arc<AFEnforcer>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        WorkspaceMemberAction::INSERT | WorkspaceMemberAction::UPDATE => match change.new {
          None => {
            warn!("The workspace member change can't be None when the action is INSERT or UPDATE")
          },
          Some(member_row) => {
            if let Err(err) = enforcer
              .update(
                &member_row.uid,
                &ObjectType::Workspace(&member_row.workspace_id.to_string()),
                &ActionType::Role(AFRole::from(member_row.role_id as i32)),
              )
              .await
            {
              error!(
                "Failed to update the user:{} workspace:{} access control, error: {}",
                member_row.uid, member_row.workspace_id, err
              );
            }
          },
        },
        WorkspaceMemberAction::DELETE => match change.old {
          None => warn!("The workspace member change can't be None when the action is DELETE"),
          Some(member_row) => {
            if let Err(err) = enforcer
              .remove(
                &member_row.uid,
                &ObjectType::Workspace(&member_row.workspace_id.to_string()),
              )
              .await
            {
              error!(
                "Failed to remove the user:{} workspace: {} access control, error: {}",
                member_row.uid, member_row.workspace_id, err
              );
            }
          },
        },
      }
    }
  });
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Clone, Debug)]
pub enum WorkspaceMemberAction {
  INSERT,
  UPDATE,
  DELETE,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WorkspaceMemberNotification {
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

pub type WorkspaceMemberListener = PostgresDBListener<WorkspaceMemberNotification>;

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
