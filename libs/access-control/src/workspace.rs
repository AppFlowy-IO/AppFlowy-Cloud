use crate::act::Action;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFRole;
use sqlx::types::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn enforce_role(
    &self,
    uid: &i64,
    workspace_id: &str,
    role: AFRole,
  ) -> Result<bool, AppError>;

  async fn enforce_action(
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
