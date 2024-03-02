use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::middleware::access_control_mw::{AccessResource, MiddlewareAccessControl};
use actix_router::{Path, ResourceDef, Url};
use actix_web::http::Method;
use app_error::AppError;
use async_trait::async_trait;
use database::collab::{is_collab_exists, CollabStorageAccessControl};

use database_entity::dto::AFAccessLevel;
use realtime::collaborate::CollabAccessControl;

use crate::api::workspace::COLLAB_PATTERN;
use crate::biz::collab::mem_cache::CollabMemCache;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::instrument;

#[derive(Clone)]
pub struct CollabMiddlewareAccessControl<AC: CollabAccessControl> {
  pub access_control: Arc<AC>,
  pg_pool: PgPool,
  skip_resources: Vec<(Method, ResourceDef)>,
}

impl<AC> CollabMiddlewareAccessControl<AC>
where
  AC: CollabAccessControl,
{
  pub fn new(access_control: Arc<AC>, pg_pool: PgPool) -> Self {
    Self {
      skip_resources: vec![
        // Skip access control when the request is a POST request and the path is matched with the COLLAB_PATTERN,
        (Method::POST, ResourceDef::new(COLLAB_PATTERN)),
      ],
      access_control,
      pg_pool,
    }
  }

  fn should_skip(&self, method: &Method, path: &Path<Url>) -> bool {
    self.skip_resources.iter().any(|(m, r)| {
      if m != method {
        return false;
      }

      r.is_match(path.as_str())
    })
  }
}

#[async_trait]
impl<AC> MiddlewareAccessControl for CollabMiddlewareAccessControl<AC>
where
  AC: CollabAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  #[instrument(name = "check_collab_permission", level = "trace", skip_all, err)]
  async fn check_resource_permission(
    &self,
    uid: &i64,
    oid: &str,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError> {
    if self.should_skip(&method, path) {
      return Ok(());
    }

    let collab_exists = is_collab_exists(oid, &self.pg_pool).await?;
    if !collab_exists {
      return Err(AppError::RecordNotFound(format!(
        "Collab not exist in db. {}",
        oid
      )));
    }

    if self
      .access_control
      .can_access_http_method(uid, oid, &method)
      .await?
    {
      Ok(())
    } else {
      Err(AppError::NotEnoughPermissions {
        user: uid.to_string(),
        action: format!(
          "access collab:{} with url:{}, method:{}",
          oid,
          path.as_str(),
          method
        ),
      })
    }
  }
}

#[derive(Clone)]
pub struct CollabStorageAccessControlImpl<CollabAC, WorkspaceAC> {
  pub(crate) collab_access_control: Arc<CollabAC>,
  pub(crate) workspace_access_control: Arc<WorkspaceAC>,
  pub(crate) pg_pool: PgPool,
}

#[async_trait]
impl<CollabAC, WorkspaceAC> CollabStorageAccessControl
  for CollabStorageAccessControlImpl<CollabAC, WorkspaceAC>
where
  CollabAC: CollabAccessControl,
  WorkspaceAC: WorkspaceAccessControl,
{
  async fn update_policy(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    self
      .collab_access_control
      .update_access_level_policy(uid, oid, level)
      .await
  }

  async fn enforce_read_collab(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    let collab_exists = is_collab_exists(oid, &self.pg_pool).await?;
    if !collab_exists {
      return Err(AppError::RecordNotFound(format!(
        "Collab not exist in db. {}",
        oid
      )));
    }
    self.collab_access_control.enforce_read(uid, oid).await
  }

  async fn enforce_write_collab(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    let collab_exists = is_collab_exists(oid, &self.pg_pool).await?;
    if !collab_exists {
      return Err(AppError::RecordNotFound(format!(
        "Collab not exist in db. {}",
        oid
      )));
    }
    self.collab_access_control.enforce_write(uid, oid).await
  }

  async fn enforce_delete_delete(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self.collab_access_control.enforce_delete(uid, oid).await
  }

  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &str) -> Result<bool, AppError> {
    self
      .workspace_access_control
      .enforce_write(uid, workspace_id)
      .await
  }
}
