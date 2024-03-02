#![allow(unused)]
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, MiddlewareAccessControl};
use actix_http::Method;
use async_trait::async_trait;
use database::user::select_uid_from_uuid;

use sqlx::{Executor, PgPool, Postgres};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::api::workspace::WORKSPACE_PATTERN;
use crate::state::UserCache;
use actix_router::{Path, ResourceDef, Url};
use anyhow::anyhow;
use app_error::AppError;
use database_entity::dto::AFRole;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, instrument, trace, warn};
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn enforce_write(&self, uid: &i64, workspace_id: &str) -> Result<bool, AppError>;

  async fn enforce_read(&self, uid: &i64, workspace_id: &str) -> Result<bool, AppError>;

  async fn insert_role(&self, uid: &i64, workspace_id: &Uuid, role: AFRole)
    -> Result<(), AppError>;

  async fn remove_role(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError>;
}

#[derive(Clone)]
pub struct WorkspaceMiddlewareAccessControl<AC: WorkspaceAccessControl> {
  pub pg_pool: PgPool,
  pub access_control: Arc<AC>,
  skip_resources: Vec<(Method, ResourceDef)>,
}

impl<AC> WorkspaceMiddlewareAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  pub fn new(pg_pool: PgPool, access_control: Arc<AC>) -> Self {
    Self {
      pg_pool,
      skip_resources: vec![
        // Skip access control when the request is a POST request and the path is matched with the WORKSPACE_PATTERN,
        (Method::POST, ResourceDef::new(WORKSPACE_PATTERN)),
      ],
      access_control,
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
impl<AC> MiddlewareAccessControl for WorkspaceMiddlewareAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Workspace
  }

  #[instrument(name = "check_workspace_permission", level = "trace", skip_all)]
  async fn check_resource_permission(
    &self,
    uid: &i64,
    resource_id: &str,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError> {
    if self.should_skip(&method, path) {
      return Ok(());
    }

    let result = match method {
      Method::DELETE | Method::POST | Method::PUT => {
        self.access_control.enforce_write(uid, resource_id).await
      },
      _ => self.access_control.enforce_read(uid, resource_id).await,
    }?;

    if result {
      Ok(())
    } else {
      Err(AppError::NotEnoughPermissions {
        user: uid.to_string(),
        action: format!(
          "access workspace:{} with given url:{}",
          resource_id,
          path.as_str()
        ),
      })
    }
  }
}
