use crate::biz::collab::ops::require_user_can_edit;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessControlService, AccessResource};
use anyhow::Error;
use async_trait::async_trait;
use realtime::collaborate::CollabPermission;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabAccessControl;

#[async_trait]
impl AccessControlService for CollabAccessControl {
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  async fn check_collab_permission(
    &self,
    workspace_id: &Uuid,
    oid: &str,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    trace!(
      "Collab access control: oid: {:?}, user_uuid: {:?}",
      oid,
      user_uuid
    );
    require_user_can_edit(pg_pool, workspace_id, user_uuid, oid).await
  }
}

/// Use to check if the user is allowed to send or receive the [CollabMessage]
pub struct CollabPermissionImpl {
  pg_pool: PgPool,
}

impl CollabPermissionImpl {
  pub fn new(pg_pool: PgPool) -> Self {
    Self { pg_pool }
  }
}

#[async_trait]
impl CollabPermission for CollabPermissionImpl {
  #[inline]
  async fn is_allowed_send_by_user(&self, uid: i64) -> Result<bool, Error> {
    todo!()
  }

  #[inline]
  async fn is_allowed_recv_by_user(&self, uid: i64) -> Result<bool, Error> {
    todo!()
  }
}
