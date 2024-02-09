use crate::biz::casbin::access_control::{enforcer_remove, enforcer_update};
use crate::biz::casbin::access_control::{ActionType, ObjectType};
use crate::biz::collab::member_listener::{CollabMemberAction, CollabMemberNotification};
use crate::biz::workspace::member_listener::{WorkspaceMemberAction, WorkspaceMemberNotification};
use database::workspace::select_permission;
use database_entity::dto::AFRole;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::error;
use tracing::log::warn;

pub(crate) fn spawn_listen_on_collab_member_change(
  pg_pool: PgPool,
  mut listener: broadcast::Receiver<CollabMemberNotification>,
  enforcer: Arc<RwLock<casbin::Enforcer>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
          if let Some(member_row) = change.new {
            if let Ok(Some(row)) = select_permission(&pg_pool, &member_row.permission_id).await {
              if let Err(err) = enforcer_update(
                &enforcer,
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
            let mut enforcer = enforcer.write().await;
            if let Err(err) = enforcer_remove(&mut enforcer, uid, &ObjectType::Collab(oid)).await {
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
  enforcer: Arc<RwLock<casbin::Enforcer>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        WorkspaceMemberAction::INSERT | WorkspaceMemberAction::UPDATE => match change.new {
          None => {
            warn!("The workspace member change can't be None when the action is INSERT or UPDATE")
          },
          Some(member_row) => {
            if let Err(err) = enforcer_update(
              &enforcer,
              &member_row.uid,
              &ObjectType::Workspace(&member_row.workspace_id.to_string()),
              &ActionType::Role(AFRole::from(member_row.role_id)),
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
            let mut enforcer = enforcer.write().await;
            if let Err(err) = enforcer_remove(
              &mut enforcer,
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
