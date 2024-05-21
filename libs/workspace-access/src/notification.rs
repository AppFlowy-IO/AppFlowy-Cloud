use access_control::access::{AccessControl, ObjectType};
use access_control::act::ActionVariant;
use database_entity::dto::AFRole;
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::error;
use tracing::log::warn;
use uuid::Uuid;

#[allow(dead_code)]
pub fn spawn_listen_on_workspace_member_change(
  mut listener: broadcast::Receiver<WorkspaceMemberNotification>,
  access_control: AccessControl,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        WorkspaceMemberAction::INSERT | WorkspaceMemberAction::UPDATE => match change.new {
          None => {
            warn!("The workspace member change can't be None when the action is INSERT or UPDATE")
          },
          Some(member_row) => {
            if let Err(err) = access_control
              .update_policy(
                &member_row.uid,
                ObjectType::Workspace(&member_row.workspace_id.to_string()),
                ActionVariant::FromRole(&AFRole::from(member_row.role_id as i32)),
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
            if let Err(err) = access_control
              .remove_policy(
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
