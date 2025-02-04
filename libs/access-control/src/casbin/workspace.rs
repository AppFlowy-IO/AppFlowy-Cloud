use async_trait::async_trait;
use tracing::instrument;
use uuid::Uuid;

use super::access::AccessControl;
use crate::act::{Action, ActionVariant};
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
  async fn enforce_role(
    &self,
    uid: &i64,
    workspace_id: &str,
    role: AFRole,
  ) -> Result<(), AppError> {
    self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&role),
      )
      .await
  }

  async fn has_role(&self, uid: &i64, workspace_id: &str, role: AFRole) -> Result<bool, AppError> {
    let result = self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&role),
      )
      .await;
    match result {
      Ok(_) => Ok(true),
      Err(AppError::NotEnoughPermissions {
        user: _,
        workspace_id: _,
      }) => Ok(false),
      Err(e) => Err(e),
    }
  }

  async fn enforce_action(
    &self,
    uid: &i64,
    workspace_id: &str,
    action: Action,
  ) -> Result<(), AppError> {
    self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromAction(&action),
      )
      .await
  }

  async fn is_action_allowed(
    &self,
    uid: &i64,
    workspace_id: &str,
    action: Action,
  ) -> Result<bool, AppError> {
    let result = self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromAction(&action),
      )
      .await;
    match result {
      Ok(_) => Ok(true),
      Err(AppError::NotEnoughPermissions {
        user: _,
        workspace_id: _,
      }) => Ok(false),
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
        ObjectType::Workspace(&workspace_id.to_string()),
        ActionVariant::FromRole(&role),
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
        &SubjectType::User(*uid),
        &ObjectType::Workspace(&workspace_id.to_string()),
      )
      .await?;

    self
      .access_control
      .remove_policy(
        &SubjectType::User(*uid),
        &ObjectType::Collab(&workspace_id.to_string()),
      )
      .await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use database_entity::dto::AFRole;

  use crate::{
    act::ActionVariant,
    casbin::{access::AccessControl, enforcer::tests::test_enforcer},
    entity::{ObjectType, SubjectType},
    workspace::WorkspaceAccessControl,
  };

  #[tokio::test]
  pub async fn test_workspace_access_control() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();
    let access_control = AccessControl::with_enforcer(enforcer);
    let workspace_access_control = super::WorkspaceAccessControlImpl::new(access_control);
    workspace_access_control
      .enforce_role(&uid, workspace_id, AFRole::Member)
      .await
      .unwrap();
    workspace_access_control
      .enforce_action(&uid, workspace_id, crate::act::Action::Read)
      .await
      .unwrap();
    assert!(workspace_access_control
      .is_action_allowed(&uid, workspace_id, crate::act::Action::Read)
      .await
      .unwrap());
    assert!(workspace_access_control
      .has_role(&uid, workspace_id, AFRole::Member)
      .await
      .unwrap());
  }
}
