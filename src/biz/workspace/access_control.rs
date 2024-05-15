#![allow(unused)]

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use actix_http::Method;
use actix_router::{Path, ResourceDef, Url};
use anyhow::anyhow;
use async_trait::async_trait;
use sqlx::{Executor, PgPool, Postgres};
use tokio::sync::{broadcast, RwLock};
use tracing::{error, instrument, trace, warn};
use uuid::Uuid;

use access_control::act::Action;
use access_control::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database::user::select_uid_from_uuid;
use database_entity::dto::AFRole;

use crate::api::workspace::{
  WORKSPACE_INVITE_PATTERN, WORKSPACE_MEMBER_PATTERN, WORKSPACE_PATTERN,
};
use crate::middleware::access_control_mw::{AccessResource, MiddlewareAccessControl};
use crate::state::UserCache;

#[derive(Clone)]
pub struct WorkspaceMiddlewareAccessControl<AC: WorkspaceAccessControl> {
  pub pg_pool: PgPool,
  pub access_control: Arc<AC>,
  skip_resources: Vec<(Method, ResourceDef)>,
  require_role_rules: Vec<(ResourceDef, HashMap<Method, AFRole>)>,
}

impl<AC> WorkspaceMiddlewareAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  pub fn new(pg_pool: PgPool, access_control: Arc<AC>) -> Self {
    Self {
      pg_pool,
      // Skip access control when the request matches the following resources
      skip_resources: vec![
        // Skip access control when the request is a POST request and the path is matched with the WORKSPACE_PATTERN,
        (Method::POST, ResourceDef::new(WORKSPACE_PATTERN)),
      ],
      // Require role for given resources
      require_role_rules: vec![
        // Only the Owner can manager the workspace members
        (
          ResourceDef::new(WORKSPACE_MEMBER_PATTERN),
          [
            (Method::POST, AFRole::Owner),
            (Method::DELETE, AFRole::Owner),
            (Method::PUT, AFRole::Owner),
            (Method::GET, AFRole::Member),
          ]
          .into(),
        ),
        (
          // Only the Owner can invite a user to the workspace
          ResourceDef::new(WORKSPACE_INVITE_PATTERN),
          [(Method::POST, AFRole::Owner)].into(),
        ),
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

  fn require_role(&self, method: &Method, path: &Path<Url>) -> Option<AFRole> {
    self.require_role_rules.iter().find_map(|(r, roles)| {
      if r.is_match(path.as_str()) {
        roles.get(method).cloned()
      } else {
        None
      }
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
    workspace_id: &str,
    uid: &i64,
    resource_id: &str,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError> {
    if self.should_skip(&method, path) {
      trace!("Skip access control for the request");
      return Ok(());
    }

    // For some specific resources, we require a specific role to access them instead of the action.
    // For example, Both AFRole::Owner and AFRole::Member have the write permission to the workspace,
    // but only the Owner can manage the workspace members.
    let require_role = self.require_role(&method, path);
    let result = match require_role {
      Some(role) => {
        self
          .access_control
          .enforce_role(uid, resource_id, role)
          .await
      },
      None => {
        // If the request doesn't match any specific resources, we enforce the action.
        let action = Action::from(&method);
        self
          .access_control
          .enforce_action(uid, resource_id, action)
          .await
      },
    }?;

    if result {
      Ok(())
    } else {
      Err(AppError::NotEnoughPermissions {
        user: uid.to_string(),
        action: format!(
          "access workspace:{} with given url:{}, method: {}",
          resource_id,
          path.as_str(),
          method,
        ),
      })
    }
  }
}
