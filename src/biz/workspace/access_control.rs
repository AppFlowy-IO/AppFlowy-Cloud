use crate::biz::workspace::ops::require_user_is_workspace_owner;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::AccessControlService;
use async_trait::async_trait;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::{debug};
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkspaceOwnerAccessControl;

#[async_trait]
impl AccessControlService for WorkspaceOwnerAccessControl {
  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    let result = require_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await;
    if let Err(err) = result.as_ref() {
      debug!("Workspace access control: {:?}", err);
    }
    result
  }
}
