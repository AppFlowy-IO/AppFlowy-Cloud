use crate::biz::collab::ops::require_user_can_edit;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::AccessControlService;
use async_trait::async_trait;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabAccessControl;

#[async_trait]
impl AccessControlService for CollabAccessControl {
  async fn check_collab_permission(
    &self,
    workspace_id: &Uuid,
    oid: &Uuid,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    trace!(
      "Collab access control: workspace_id: {:?}, oid: {:?}, user_uuid: {:?}",
      workspace_id,
      oid,
      user_uuid
    );
    require_user_can_edit(pg_pool, user_uuid, oid, workspace_id).await
  }
}
