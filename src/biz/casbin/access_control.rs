use crate::biz::casbin::collab_ac::CollabAccessControlImpl;
use crate::biz::casbin::enforcer::AFEnforcer;
use crate::biz::casbin::pg_listen::*;
use crate::biz::casbin::workspace_ac::WorkspaceAccessControlImpl;

use app_error::AppError;
use casbin::Enforcer;
use database_entity::dto::{AFAccessLevel, AFRole};

use actix_http::Method;
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
}

impl AccessControl {
  pub fn new(
    pg_pool: PgPool,
    collab_listener: broadcast::Receiver<CollabMemberNotification>,
    workspace_listener: broadcast::Receiver<WorkspaceMemberNotification>,
    enforcer: Enforcer,
  ) -> Self {
    let enforcer = Arc::new(AFEnforcer::new(enforcer));
    spawn_listen_on_workspace_member_change(workspace_listener, enforcer.clone());
    spawn_listen_on_collab_member_change(pg_pool, collab_listener, enforcer.clone());
    Self { enforcer }
  }
  pub fn new_collab_access_control(&self) -> CollabAccessControlImpl {
    CollabAccessControlImpl::new(self.clone())
  }

  pub fn new_workspace_access_control(&self) -> WorkspaceAccessControlImpl {
    WorkspaceAccessControlImpl::new(self.clone())
  }

  pub async fn contains(&self, obj: &ObjectType<'_>) -> bool {
    self.enforcer.contains(obj).await
  }

  pub async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    self.enforcer.update(uid, obj, act).await
  }

  pub async fn remove(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<(), AppError> {
    self.enforcer.remove(uid, obj).await?;
    Ok(())
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToCasbinAction,
  {
    self.enforcer.enforce(uid, obj, act).await
  }

  pub async fn get_access_level(&self, uid: &i64, oid: &str) -> Option<AFAccessLevel> {
    let collab_id = ObjectType::Collab(oid);
    self
      .enforcer
      .get_action(uid, &collab_id)
      .await
      .map(|value| AFAccessLevel::from_action(&value))
  }

  pub async fn get_role(&self, uid: &i64, workspace_id: &str) -> Option<AFRole> {
    let workspace_id = ObjectType::Workspace(workspace_id);
    self
      .enforcer
      .get_action(uid, &workspace_id)
      .await
      .map(|value| AFRole::from_action(&value))
  }
}

pub const MODEL_CONF: &str = r###"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # rule for action
g2 = _, _ # rule for collab object id

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && g2(p.obj, r.obj) && g(p.act, r.act)
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
