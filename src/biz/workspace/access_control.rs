use crate::biz::workspace::member_listener::WorkspaceMemberChange;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_http::Method;
use anyhow::Context;
use async_trait::async_trait;
use database::user::select_uid_from_uuid;
use database::workspace::select_user_role;
use database_entity::AFRole;
use shared_entity::app_error::AppError;
use shared_entity::error_code::ErrorCode;
use sqlx::PgPool;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{instrument, trace};
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn get_role_from_uuid(
    &self,
    user_uuid: &Uuid,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError>;
  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError>;
}

pub struct WorkspaceAccessControlImpl {
  pg_pool: PgPool,
}

impl WorkspaceAccessControlImpl {
  pub fn new(pg_pool: PgPool, _listener: broadcast::Receiver<WorkspaceMemberChange>) -> Self {
    WorkspaceAccessControlImpl { pg_pool }
  }
}

#[async_trait]
impl WorkspaceAccessControl for WorkspaceAccessControlImpl {
  async fn get_role_from_uuid(
    &self,
    user_uuid: &Uuid,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError> {
    let mut txn = self
      .pg_pool
      .begin()
      .await
      .context("failed to acquire a transaction to query role")?;
    let uid = select_uid_from_uuid(txn.deref_mut(), user_uuid).await?;
    let role = select_user_role(txn.deref_mut(), &uid, workspace_id).await?;
    txn
      .commit()
      .await
      .context("failed to commit transaction to query role")?;
    Ok(role)
  }

  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError> {
    let role = select_user_role(&self.pg_pool, uid, workspace_id).await?;
    Ok(role)
  }
}

#[derive(Clone)]
pub struct WorkspaceHttpAccessControl<AC: WorkspaceAccessControl>(pub Arc<AC>);
#[async_trait]
impl<AC> HttpAccessControlService for WorkspaceHttpAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Workspace
  }

  #[instrument(level = "trace", skip_all, err)]
  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    method: Method,
  ) -> Result<(), AppError> {
    trace!(
      "workspace_id: {:?}, user_uuid: {:?}",
      workspace_id,
      user_uuid
    );

    match self.0.get_role_from_uuid(user_uuid, workspace_id).await {
      Ok(role) => {
        if method == Method::DELETE || method == Method::POST || method == Method::PUT {
          if matches!(role, AFRole::Owner) {
            Ok(())
          } else {
            Err(AppError::new(
              ErrorCode::NotEnoughPermissions,
              format!(
                "User:{:?} doesn't have the enough permission to access workspace:{}",
                user_uuid, workspace_id
              ),
            ))
          }
        } else {
          Ok(())
        }
      },
      Err(err) => Err(AppError::new(
        ErrorCode::NotEnoughPermissions,
        err.to_string(),
      )),
    }
  }
}
