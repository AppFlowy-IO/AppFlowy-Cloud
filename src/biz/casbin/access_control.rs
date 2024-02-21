use crate::biz::casbin::collab_ac::CollabAccessControlImpl;
use crate::biz::casbin::enforcer::AFEnforcer;
use crate::biz::casbin::pg_listen::*;
use crate::biz::casbin::workspace_ac::WorkspaceAccessControlImpl;

use app_error::AppError;
use casbin::CoreApi;
use database_entity::dto::{AFAccessLevel, AFRole};

use crate::biz::casbin::adapter::PgAdapter;
use crate::biz::casbin::metrics::AccessControlMetrics;
use actix_http::Method;
use anyhow::anyhow;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;

use tokio::sync::broadcast;

/// Manages access control.
///
/// Stores access control policies in the form `subject, object, role`
/// where `subject` is `uid`, `object` is `oid`, and `role` is [AFAccessLevel] or [AFRole].
///
/// Roles are mapped to the corresponding actions that they are allowed to perform.
/// `FullAccess` has write
/// `FullAccess` has read
///
/// Access control requests are made in the form `subject, object, action`
/// and will be evaluated against the policies and mappings stored,
/// according to the model defined.
#[derive(Clone)]
pub struct AccessControl {
  enforcer: Arc<AFEnforcer>,
  #[allow(dead_code)]
  access_control_metrics: Arc<AccessControlMetrics>,
}

impl AccessControl {
  pub async fn new(
    pg_pool: PgPool,
    collab_listener: broadcast::Receiver<CollabMemberNotification>,
    workspace_listener: broadcast::Receiver<WorkspaceMemberNotification>,
    access_control_metrics: Arc<AccessControlMetrics>,
  ) -> Result<Self, AppError> {
    let enforcer_result_cache = Arc::new(DashMap::new());
    let action_cache = Arc::new(DashMap::new());

    let access_control_model = casbin::DefaultModel::from_str(MODEL_CONF)
      .await
      .map_err(|e| AppError::Internal(anyhow!("Failed to create access control model: {}", e)))?;
    let access_control_adapter = PgAdapter::new(
      pg_pool.clone(),
      action_cache.clone(),
      access_control_metrics.clone(),
    );
    let enforcer = casbin::Enforcer::new(access_control_model, access_control_adapter)
      .await
      .map_err(|e| {
        AppError::Internal(anyhow!("Failed to create access control enforcer: {}", e))
      })?;

    let enforcer = Arc::new(AFEnforcer::new(
      enforcer,
      enforcer_result_cache,
      action_cache,
    ));
    spawn_listen_on_workspace_member_change(workspace_listener, enforcer.clone());
    spawn_listen_on_collab_member_change(pg_pool, collab_listener, enforcer.clone());
    Ok(Self {
      enforcer,
      access_control_metrics,
    })
  }
  pub fn new_collab_access_control(&self) -> CollabAccessControlImpl {
    CollabAccessControlImpl::new(self.clone())
  }

  pub fn new_workspace_access_control(&self) -> WorkspaceAccessControlImpl {
    WorkspaceAccessControlImpl::new(self.clone())
  }

  pub async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self.enforcer.update(uid, obj, act).await
    }
  }

  pub async fn remove(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<(), AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(())
    } else {
      self.enforcer.remove(uid, obj).await?;
      Ok(())
    }
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToCasbinAction,
  {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self.enforcer.enforce(uid, obj, act).await
    }
  }

  pub async fn get_access_level(&self, uid: &i64, oid: &str) -> Option<AFAccessLevel> {
    if cfg!(feature = "disable_access_control") {
      Some(AFAccessLevel::FullAccess)
    } else {
      let collab_id = ObjectType::Collab(oid);
      self
        .enforcer
        .get_action(uid, &collab_id)
        .await
        .map(|value| AFAccessLevel::from_action(&value))
    }
  }

  pub async fn get_role(&self, uid: &i64, workspace_id: &str) -> Option<AFRole> {
    if cfg!(feature = "disable_access_control") {
      Some(AFRole::Owner)
    } else {
      let workspace_id = ObjectType::Workspace(workspace_id);
      self
        .enforcer
        .get_action(uid, &workspace_id)
        .await
        .map(|value| AFRole::from_action(&value))
    }
  }
}

/// policy in db:
///   p = 1, 123, 1 (1 mean AFRole::Owner)
///   p = 1, 456, 50 (50 mean AFAccessLevel::FullAccess)
///
/// role_definition in db:
///   g = _, _
///      af role:
///      ["1", "delete"], ["1", "write"], ["1", "read"],
///      ["2", "write"], ["2", "read"],
///      ["3", "read"],
///      af access level:
///      ["10", "read"],
///      ["20", "read"],
///      ["30", "read"], ["30", "write"],
///      ["50", "read"], ["50", "write"], ["50", "delete"]
///
/// matchers:
/// r.sub == p.sub && p.obj == r.obj && g(p.act, r.act)
///
/// Example:
///   request:
///    1. api/workspace/123, user=1, workspace_id=123 GET
///     r = sub = 1, obj = 123, act =read
///     p = sub = 1, obj = 123, act = 1
///
///    Evaluation:
///     1. Subject Match: r.sub == p.sub
///     2. Object Match: p.obj == r.obj
///     3. Action Permission: g(p.act, r.act) => g(1, read) =>  ["1", "read"]
///    Result:
///     Allow
///
pub const MODEL_CONF: &str = r###"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # rule for action

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && p.obj == r.obj && g(p.act, r.act)
"###;

/// Represents the entity stored at the index of the access control policy.
/// `user_id, object_id, role/action`
///
/// E.g. user1, collab::123, Owner
///
pub const POLICY_FIELD_INDEX_USER: usize = 0;
pub const POLICY_FIELD_INDEX_OBJECT: usize = 1;
pub const POLICY_FIELD_INDEX_ACTION: usize = 2;

/// Represents the entity stored at the index of the grouping.
/// `role, action`
///
/// E.g. Owner, Write
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ROLE: usize = 0;
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ACTION: usize = 1;

/// Represents the object type that is stored in the access control policy.
#[derive(Debug)]
pub enum ObjectType<'id> {
  /// Stored as `workspace::<uuid>`
  Workspace(&'id str),
  /// Stored as `collab::<uuid>`
  Collab(&'id str),
}

impl ObjectType<'_> {
  pub fn to_object_id(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }
}

/// Represents the action type that is stored in the access control policy.
#[derive(Debug)]
pub enum ActionType {
  Role(AFRole),
  Level(AFAccessLevel),
}

impl ToCasbinAction for ActionType {
  fn to_action(&self) -> String {
    match self {
      ActionType::Role(role) => role.to_action(),
      ActionType::Level(level) => level.to_action(),
    }
  }
}

/// Represents the actions that can be performed on objects.
#[derive(Debug)]
pub enum Action {
  Read,
  Write,
  Delete,
}

impl ToCasbinAction for Action {
  fn to_action(&self) -> String {
    match self {
      Action::Read => "read".to_owned(),
      Action::Write => "write".to_owned(),
      Action::Delete => "delete".to_owned(),
    }
  }
}

impl From<Method> for Action {
  fn from(method: Method) -> Self {
    Self::from(&method)
  }
}

impl From<&Method> for Action {
  fn from(method: &Method) -> Self {
    match *method {
      Method::POST => Action::Write,
      Method::PUT => Action::Write,
      Method::DELETE => Action::Delete,
      _ => Action::Read,
    }
  }
}

pub trait ToCasbinAction {
  fn to_action(&self) -> String;
}
pub trait FromCasbinAction {
  fn from_action(action: &str) -> Self;
}

impl ToCasbinAction for AFAccessLevel {
  fn to_action(&self) -> String {
    i32::from(self).to_string()
  }
}

impl FromCasbinAction for AFAccessLevel {
  fn from_action(action: &str) -> Self {
    Self::from(action)
  }
}

impl ToCasbinAction for AFRole {
  fn to_action(&self) -> String {
    i32::from(self).to_string()
  }
}
impl FromCasbinAction for AFRole {
  fn from_action(action: &str) -> Self {
    Self::from(action)
  }
}
