use std::collections::HashMap;
use std::sync::Arc;

use actix_router::{Path, ResourceDef, Url};
use actix_web::http::Method;
use async_trait::async_trait;
use tracing::{instrument, trace};

use access_control::act::Action;
use access_control::collab::CollabAccessControl;
use app_error::AppError;
use appflowy_collaborate::collab::cache::CollabCache;
use database_entity::dto::AFAccessLevel;

use crate::api::workspace::{COLLAB_PATTERN, V1_COLLAB_PATTERN};
use crate::middleware::access_control_mw::{AccessResource, MiddlewareAccessControl};

#[derive(Clone)]
pub struct CollabMiddlewareAccessControl<AC: CollabAccessControl> {
  pub access_control: Arc<AC>,
  collab_cache: CollabCache,
  skip_resources: Vec<(Method, ResourceDef)>,
  require_access_levels: Vec<(ResourceDef, HashMap<Method, AFAccessLevel>)>,
}

impl<AC> CollabMiddlewareAccessControl<AC>
where
  AC: CollabAccessControl,
{
  pub fn new(access_control: Arc<AC>, collab_cache: CollabCache) -> Self {
    Self {
      skip_resources: vec![
        // Skip access control when trying to create a collab
        (Method::POST, ResourceDef::new(COLLAB_PATTERN)),
        (Method::POST, ResourceDef::new(V1_COLLAB_PATTERN)),
      ],
      require_access_levels: vec![
        (
          ResourceDef::new(COLLAB_PATTERN),
          [
            // Only the user with FullAccess can delete the collab
            (Method::DELETE, AFAccessLevel::FullAccess),
          ]
          .into(),
        ),
        (
          ResourceDef::new(V1_COLLAB_PATTERN),
          [
            // Only the user with FullAccess can delete the collab
            (Method::DELETE, AFAccessLevel::FullAccess),
          ]
          .into(),
        ),
      ],
      access_control,
      collab_cache,
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

  fn require_access_level(&self, method: &Method, path: &Path<Url>) -> Option<AFAccessLevel> {
    self.require_access_levels.iter().find_map(|(r, roles)| {
      if r.is_match(path.as_str()) {
        roles.get(method).cloned()
      } else {
        None
      }
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

  #[instrument(name = "check_collab_permission", level = "trace", skip_all)]
  async fn check_resource_permission(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError> {
    if self.should_skip(&method, path) {
      trace!("Skip access control for the request");
      return Ok(());
    }
    let collab_exists = self.collab_cache.is_exist(oid).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control
      return Ok(());
    }

    let access_level = self.require_access_level(&method, path);
    let result = match access_level {
      None => {
        self
          .access_control
          .enforce_action(workspace_id, uid, oid, Action::from(&method))
          .await?
      },
      Some(access_level) => {
        self
          .access_control
          .enforce_access_level(workspace_id, uid, oid, access_level)
          .await?
      },
    };

    if result {
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
