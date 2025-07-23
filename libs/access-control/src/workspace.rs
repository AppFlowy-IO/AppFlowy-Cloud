use crate::act::Action;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFRole;
use sqlx::types::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  /// Check if the user has the role in the workspace.
  /// Returns AppError::NotEnoughPermission if the user does not have the role.
  async fn enforce_role_strong(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError>;

  async fn enforce_role_weak(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError>;

  /// Check if the user can perform action on the workspace.
  /// Returns AppError::NotEnoughPermission if the user does not have the role.
  async fn enforce_action(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    action: Action,
  ) -> Result<(), AppError>;

  async fn insert_role(&self, uid: &i64, workspace_id: &Uuid, role: AFRole)
    -> Result<(), AppError>;

  async fn remove_user_from_workspace(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
  ) -> Result<(), AppError>;
}
