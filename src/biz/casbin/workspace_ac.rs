use crate::biz::casbin::access_control::AccessControl;
use crate::biz::casbin::access_control::{ActionType, ObjectType};
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use app_error::AppError;
use async_trait::async_trait;

use database_entity::dto::AFRole;
use sqlx::{Executor, Postgres};

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
  async fn get_workspace_role<'a, E>(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    _executor: E,
  ) -> Result<AFRole, AppError>
  where
    E: Executor<'a, Database = Postgres>,
  {
    let workspace_id = workspace_id.to_string();
    self
      .access_control
      .get_role(uid, &workspace_id)
      .await
      .ok_or_else(|| {
        AppError::RecordNotFound(format!(
          "can't find the role for user:{} workspace:{}",
          uid, workspace_id
        ))
      })
  }

  #[instrument(level = "info", skip_all)]
  async fn insert_workspace_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let _ = self
      .access_control
      .update(
        uid,
        &ObjectType::Workspace(&workspace_id.to_string()),
        &ActionType::Role(role),
      )
      .await?;
    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_role(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError> {
    let _ = self
      .access_control
      .remove(uid, &ObjectType::Workspace(&workspace_id.to_string()))
      .await?;
    Ok(())
  }
}
