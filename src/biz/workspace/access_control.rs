#![allow(unused)]
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_http::Method;
use async_trait::async_trait;
use database::user::select_uid_from_uuid;

use sqlx::{Executor, PgPool, Postgres};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use actix_router::{Path, Url};
use anyhow::anyhow;
use app_error::AppError;
use database_entity::dto::AFRole;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, instrument, trace, warn};
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn get_workspace_role<'a, E>(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    executor: E,
  ) -> Result<AFRole, AppError>
  where
    E: Executor<'a, Database = Postgres>;

  async fn insert_workspace_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError>;

  async fn remove_role(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError>;
}

#[derive(Clone)]
pub struct WorkspaceHttpAccessControl<AC: WorkspaceAccessControl> {
  pub pg_pool: PgPool,
  pub access_control: Arc<AC>,
}
#[async_trait]
impl<AC> HttpAccessControlService for WorkspaceHttpAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Workspace
  }

  #[instrument(level = "trace", skip_all, err)]
  #[allow(clippy::blocks_in_if_conditions)]
  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    method: Method,
  ) -> Result<(), AppError> {
    trace!("workspace_id: {:?}, uid: {:?}", workspace_id, uid);
    let role = self
      .access_control
      .get_workspace_role(uid, workspace_id, &self.pg_pool)
      .await
      .map_err(|err| {
        AppError::NotEnoughPermissions(format!(
          "Can't find the role of the user:{:?} in the workspace:{:?}. error: {}",
          uid, workspace_id, err
        ))
      })?;

    match method {
      Method::DELETE | Method::POST | Method::PUT => match role {
        AFRole::Owner => return Ok(()),
        _ => {
          return Err(AppError::NotEnoughPermissions(format!(
            "User:{:?} doesn't have the enough permission to access workspace:{}",
            uid, workspace_id
          )))
        },
      },
      _ => Ok(()),
    }
  }

  async fn check_collab_permission(
    &self,
    oid: &str,
    uid: &i64,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError> {
    error!("The check_collab_permission is not implemented");
    Ok(())
  }
}
