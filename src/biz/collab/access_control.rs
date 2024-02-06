use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_router::{Path, Url};
use actix_web::http::Method;
use app_error::AppError;
use async_trait::async_trait;
use database::collab::CollabStorageAccessControl;

use database_entity::dto::{AFAccessLevel, AFRole};
use realtime::collaborate::CollabAccessControl;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use tracing::instrument;

#[derive(Clone)]
pub struct CollabHttpAccessControl<AC: CollabAccessControl>(pub Arc<AC>);

#[async_trait]
impl<AC> HttpAccessControlService for CollabHttpAccessControl<AC>
where
  AC: CollabAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  #[instrument(level = "debug", skip_all, err)]
  async fn check_collab_permission(
    &self,
    oid: &str,
    uid: &i64,
    method: Method,
    _path: &Path<Url>,
  ) -> Result<(), AppError> {
    let can_access = self.0.can_access_http_method(uid, oid, &method).await?;

    if !can_access {
      return Err(AppError::NotEnoughPermissions(format!(
        "Not enough permissions to access the collab: {} with http method: {}",
        oid, method
      )));
    }
    Ok(())
  }
}

#[derive(Clone)]
pub struct CollabStorageAccessControlImpl<CollabAC, WorkspaceAC> {
  pub(crate) collab_access_control: Arc<CollabAC>,
  pub(crate) workspace_access_control: Arc<WorkspaceAC>,
}

#[async_trait]
impl<CollabAC, WorkspaceAC> CollabStorageAccessControl
  for CollabStorageAccessControlImpl<CollabAC, WorkspaceAC>
where
  CollabAC: CollabAccessControl,
  WorkspaceAC: WorkspaceAccessControl,
{
  async fn get_collab_access_level(&self, uid: &i64, oid: &str) -> Result<AFAccessLevel, AppError> {
    self
      .collab_access_control
      .get_collab_access_level(uid, oid)
      .await
  }

  async fn cache_collab_access_level(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    self
      .collab_access_control
      .cache_collab_access_level(uid, oid, level)
      .await
  }

  async fn get_user_workspace_role<'a, E: Executor<'a, Database = Postgres>>(
    &self,
    uid: &i64,
    workspace_id: &str,
    executor: E,
  ) -> Result<AFRole, AppError> {
    self
      .workspace_access_control
      .get_role_from_uid(uid, &workspace_id.parse()?, executor)
      .await
  }
}
