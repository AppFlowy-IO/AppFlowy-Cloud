use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::AccessControlService;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use tracing::trace;

#[derive(Clone)]
pub struct WorkspaceMemberAccessControl;

impl AccessControlService for WorkspaceMemberAccessControl {
  fn check_permission(&self, _user_uuid: UserUuid, _pg_pool: &PgPool) -> Result<(), AppError> {
    trace!("workspace member access control");
    Ok(())
  }
}
