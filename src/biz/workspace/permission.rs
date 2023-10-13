use crate::biz::workspace::ops::require_user_is_workspace_owner;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::WorkspaceAccessControlService;
use async_trait::async_trait;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkspaceOwnerAccessControl;

#[async_trait]
impl WorkspaceAccessControlService for WorkspaceOwnerAccessControl {
  async fn check_permission(
    &self,
    workspace_id: Uuid,
    user_uuid: UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    let result = require_user_is_workspace_owner(pg_pool, &user_uuid, &workspace_id).await;
    trace!("Workspace owner access control: {:?}", result);
    result
  }
}

#[derive(Clone)]
pub struct WorkspaceMemberAccessControl;
#[async_trait]
impl WorkspaceAccessControlService for WorkspaceMemberAccessControl {
  async fn check_permission(
    &self,
    workspace_id: Uuid,
    user_uuid: UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    let result = require_user_is_workspace_owner(pg_pool, &user_uuid, &workspace_id).await;
    trace!("Workspace member access control: {:?}", result);
    result
  }
}
