use crate::biz::workspace::ops::require_user_is_workspace_owner;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use async_trait::async_trait;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkspaceOwnerAccessControl;
#[async_trait]
impl HttpAccessControlService for WorkspaceOwnerAccessControl {
  fn resource(&self) -> AccessResource {
    AccessResource::Workspace
  }

  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    trace!(
      "workspace_id: {:?}, user_uuid: {:?}",
      workspace_id,
      user_uuid
    );
    require_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await
  }
}
