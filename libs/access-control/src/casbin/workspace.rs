use async_trait::async_trait;
use tracing::instrument;
use uuid::Uuid;

use super::access::AccessControl;
use crate::act::Action;
use crate::entity::{ObjectType, SubjectType};
use crate::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database_entity::dto::AFRole;

#[derive(Clone)]
pub struct WorkspaceAccessControlImpl {
  access_control: AccessControl,
}

impl WorkspaceAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    Self { access_control }
  }
}

#[async_trait]
impl WorkspaceAccessControl for WorkspaceAccessControlImpl {
  async fn enforce_role_strong(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let result = self
      .access_control
      .enforce_strong(uid, ObjectType::Workspace(workspace_id.to_string()), role)
      .await;

    match result {
      Ok(true) => Ok(()),
      Ok(false) => Err(AppError::NotEnoughPermissions),
      Err(e) => Err(e),
    }
  }

  async fn enforce_role_weak(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let result = self
      .access_control
      .enforce_weak(uid, ObjectType::Workspace(workspace_id.to_string()), role)
      .await;

    match result {
      Ok(true) => Ok(()),
      Ok(false) => Err(AppError::NotEnoughPermissions),
      Err(e) => Err(e),
    }
  }

  async fn enforce_action(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    action: Action,
  ) -> Result<(), AppError> {
    let result = self
      .access_control
      .enforce_immediately(uid, ObjectType::Workspace(workspace_id.to_string()), action)
      .await;
    match result {
      Ok(true) => Ok(()),
      Ok(false) => Err(AppError::NotEnoughPermissions),
      Err(e) => Err(e),
    }
  }

  #[instrument(level = "info", skip_all)]
  async fn insert_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    self
      .access_control
      .update_policy(
        SubjectType::User(*uid),
        ObjectType::Workspace(workspace_id.to_string()),
        role,
      )
      .await?;
    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_user_from_workspace(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
  ) -> Result<(), AppError> {
    self
      .access_control
      .remove_policy(
        SubjectType::User(*uid),
        ObjectType::Workspace(workspace_id.to_string()),
      )
      .await?;

    self
      .access_control
      .remove_policy(
        SubjectType::User(*uid),
        ObjectType::Collab(workspace_id.to_string()),
      )
      .await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use app_error::ErrorCode;
  use database_entity::dto::AFRole;
  use uuid::Uuid;

  use crate::casbin::util::tests::test_enforcer_v2;
  use crate::{
    casbin::access::AccessControl,
    entity::{ObjectType, SubjectType},
    workspace::WorkspaceAccessControl,
  };

  #[tokio::test]
  pub async fn test_workspace_access_control() {
    let enforcer = test_enforcer_v2().await;
    let member_uid = 1;
    let owner_uid = 2;
    let workspace_id = Uuid::new_v4();
    enforcer
      .update_policy(
        SubjectType::User(member_uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .unwrap();
    enforcer
      .update_policy(
        SubjectType::User(owner_uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();
    let access_control = AccessControl::with_enforcer(enforcer);
    let workspace_access_control = super::WorkspaceAccessControlImpl::new(access_control);
    for uid in [member_uid, owner_uid] {
      workspace_access_control
        .enforce_role_strong(&uid, &workspace_id, AFRole::Member)
        .await
        .unwrap_or_else(|_| panic!("Failed to enforce role for {}", uid));
      workspace_access_control
        .enforce_action(&uid, &workspace_id, crate::act::Action::Read)
        .await
        .unwrap_or_else(|_| panic!("Failed to enforce action for {}", uid));
    }
    let result = workspace_access_control
      .enforce_action(&member_uid, &workspace_id, crate::act::Action::Delete)
      .await;
    let error_code = result.unwrap_err().code();
    assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
    workspace_access_control
      .enforce_action(&owner_uid, &workspace_id, crate::act::Action::Delete)
      .await
      .unwrap();
  }
}
