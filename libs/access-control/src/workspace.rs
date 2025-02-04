use crate::act::Action;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFRole;
use sqlx::types::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  /// Check if the user has the role in the workspace.
  /// Returns AppError::NotEnoughPermission if the user does not have the role.
  async fn enforce_role(&self, uid: &i64, workspace_id: &str, role: AFRole)
    -> Result<(), AppError>;

  /// Check if the user has the role in the workspace.
  /// Returns False if the user does not have the role.
  async fn has_role(&self, uid: &i64, workspace_id: &str, role: AFRole) -> Result<bool, AppError>;

  /// Check if the user can perform action on the workspace.
  /// Returns AppError::NotEnoughPermission if the user does not have the role.
  async fn enforce_action(
    &self,
    uid: &i64,
    workspace_id: &str,
    action: Action,
  ) -> Result<(), AppError>;

  /// Check if the user can perform action on the workspace.
  /// Returns false if the user is not allowed to perform the action.
  async fn is_action_allowed(
    &self,
    uid: &i64,
    workspace_id: &str,
    action: Action,
  ) -> Result<bool, AppError>;

  async fn insert_role(&self, uid: &i64, workspace_id: &Uuid, role: AFRole)
    -> Result<(), AppError>;

  async fn remove_user_from_workspace(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
  ) -> Result<(), AppError>;
}
