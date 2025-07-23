use async_trait::async_trait;
use uuid::Uuid;

use crate::act::Action;
use crate::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database_entity::dto::AFRole;

#[derive(Clone)]
pub struct WorkspaceAccessControlImpl;

impl WorkspaceAccessControlImpl {
  pub fn new() -> Self {
    Self {}
  }
}

impl Default for WorkspaceAccessControlImpl {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl WorkspaceAccessControl for WorkspaceAccessControlImpl {
  async fn enforce_role_strong(
    &self,
    _uid: &i64,
    _workspace_id: &Uuid,
    _role: AFRole,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn enforce_role_weak(
    &self,
    _uid: &i64,
    _workspace_id: &Uuid,
    _role: AFRole,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn enforce_action(
    &self,
    _uid: &i64,
    _workspace_id: &Uuid,
    _action: Action,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn insert_role(
    &self,
    _uid: &i64,
    _workspace_id: &Uuid,
    _role: AFRole,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn remove_user_from_workspace(
    &self,
    _uid: &i64,
    _workspace_id: &Uuid,
  ) -> Result<(), AppError> {
    Ok(())
  }
}
