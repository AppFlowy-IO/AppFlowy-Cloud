use crate::biz::casbin::access_control::{AccessControl, Action};
use crate::biz::casbin::access_control::{ActionType, ObjectType};
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::{AFAccessLevel, AFRole};
use tracing::instrument;
use uuid::Uuid;

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
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(uid, &ObjectType::Workspace(workspace_id), role)
      .await
  }

  async fn enforce_action(
    &self,
    uid: &i64,
    workspace_id: &str,
    action: Action,
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(uid, &ObjectType::Workspace(workspace_id), action)
      .await
  }

  #[instrument(level = "info", skip_all)]
  async fn insert_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let access_level = AFAccessLevel::from(&role);
    self
      .access_control
      .update_policy(
        uid,
        &ObjectType::Workspace(&workspace_id.to_string()),
        &ActionType::Role(role),
      )
      .await?;
    self
      .access_control
      .update_policy(
        uid,
        &ObjectType::Collab(&workspace_id.to_string()),
        &ActionType::Level(access_level),
      )
      .await?;
    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_role(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError> {
    self
      .access_control
      .remove_policy(uid, &ObjectType::Workspace(&workspace_id.to_string()))
      .await?;

    self
      .access_control
      .remove_policy(uid, &ObjectType::Collab(&workspace_id.to_string()))
      .await?;
    Ok(())
  }
}
