use crate::biz::casbin::collab_ac::CollabAccessControlImpl;
use crate::biz::casbin::enforcer::AFEnforcer;

use crate::biz::casbin::workspace_ac::WorkspaceAccessControlImpl;
use std::cmp::Ordering;

use app_error::AppError;
use casbin::CoreApi;
use database_entity::dto::{AFAccessLevel, AFRole};

use crate::biz::casbin::adapter::PgAdapter;
use crate::biz::casbin::metrics::AccessControlMetrics;
use actix_http::Method;
use anyhow::anyhow;

use sqlx::PgPool;

use lazy_static::lazy_static;
use redis::{ErrorKind, FromRedisValue, RedisError, RedisResult, RedisWrite, ToRedisArgs, Value};
use std::sync::Arc;

use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum AccessControlChange {
  UpdatePolicy { uid: i64, oid: String },
  RemovePolicy { uid: i64, oid: String },
}

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
  change_tx: broadcast::Sender<AccessControlChange>,
}

impl AccessControl {
  pub async fn new(
    pg_pool: PgPool,
    access_control_metrics: Arc<AccessControlMetrics>,
  ) -> Result<Self, AppError> {
    let access_control_model = casbin::DefaultModel::from_str(MODEL_CONF)
      .await
      .map_err(|e| AppError::Internal(anyhow!("Failed to create access control model: {}", e)))?;
    let access_control_adapter = PgAdapter::new(pg_pool.clone(), access_control_metrics.clone());
    let enforcer = casbin::Enforcer::new(access_control_model, access_control_adapter)
      .await
      .map_err(|e| {
        AppError::Internal(anyhow!("Failed to create access control enforcer: {}", e))
      })?;

    let enforcer = Arc::new(AFEnforcer::new(enforcer, access_control_metrics.clone()));
    let (change_tx, _) = broadcast::channel(1000);
    Ok(Self {
      enforcer,
      access_control_metrics,
      change_tx,
    })
  }

  pub fn subscribe_change(&self) -> broadcast::Receiver<AccessControlChange> {
    self.change_tx.subscribe()
  }

  pub fn new_collab_access_control(&self) -> CollabAccessControlImpl {
    CollabAccessControlImpl::new(self.clone())
  }

  pub fn new_workspace_access_control(&self) -> WorkspaceAccessControlImpl {
    WorkspaceAccessControlImpl::new(self.clone())
  }

  pub async fn update_policy(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<(), AppError> {
    if enable_access_control() {
      let result = self.enforcer.update_policy(uid, obj, act).await;
      let _ = self.change_tx.send(AccessControlChange::UpdatePolicy {
        uid: *uid,
        oid: obj.object_id().to_string(),
      });
      result
    } else {
      Ok(())
    }
  }

  pub async fn remove_policy(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<(), AppError> {
    if enable_access_control() {
      self.enforcer.remove_policy(uid, obj).await?;
      let _ = self.change_tx.send(AccessControlChange::RemovePolicy {
        uid: *uid,
        oid: obj.object_id().to_string(),
      });
      Ok(())
    } else {
      Ok(())
    }
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToACAction,
  {
    if enable_access_control() {
      self.enforcer.enforce_policy(uid, obj, act).await
    } else {
      Ok(true)
    }
  }
}

/// policy:
///   p = sub=uid, obj=object_id, act=role_id
///   p = sub=uid, obj=object_id, act=access_level
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
  pub fn policy_object(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }

  pub fn object_id(&self) -> &str {
    match self {
      ObjectType::Collab(s) => s,
      ObjectType::Workspace(s) => s,
    }
  }
}

/// Represents the action type that is stored in the access control policy.
#[derive(Debug)]
pub enum ActionType {
  Role(AFRole),
  Level(AFAccessLevel),
}

impl ToACAction for ActionType {
  fn to_action(&self) -> &str {
    match self {
      ActionType::Role(role) => role.to_action(),
      ActionType::Level(level) => level.to_action(),
    }
  }
}

/// Represents the actions that can be performed on objects.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Action {
  Read,
  Write,
  Delete,
}

impl PartialOrd for Action {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for Action {
  fn cmp(&self, other: &Self) -> Ordering {
    match (self, other) {
      // Read
      (Action::Read, Action::Read) => Ordering::Equal,
      (Action::Read, _) => Ordering::Less,
      (_, Action::Read) => Ordering::Greater,
      // Write
      (Action::Write, Action::Write) => Ordering::Equal,
      (Action::Write, Action::Delete) => Ordering::Less,
      // Delete
      (Action::Delete, Action::Write) => Ordering::Greater,
      (Action::Delete, Action::Delete) => Ordering::Equal,
    }
  }
}

impl ToRedisArgs for Action {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + RedisWrite,
  {
    self.to_action().write_redis_args(out)
  }
}

impl FromRedisValue for Action {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let s: String = FromRedisValue::from_redis_value(v)?;
    match s.as_str() {
      "read" => Ok(Action::Read),
      "write" => Ok(Action::Write),
      "delete" => Ok(Action::Delete),
      _ => Err(RedisError::from((ErrorKind::TypeError, "invalid action"))),
    }
  }
}

impl ToACAction for Action {
  fn to_action(&self) -> &str {
    match self {
      Action::Read => "read",
      Action::Write => "write",
      Action::Delete => "delete",
    }
  }
}

impl ToACAction for &Action {
  fn to_action(&self) -> &str {
    match self {
      Action::Read => "read",
      Action::Write => "write",
      Action::Delete => "delete",
    }
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

pub trait ToACAction {
  fn to_action(&self) -> &str;
}
pub trait FromACAction {
  fn from_action(action: &str) -> Self;
}

impl ToACAction for AFAccessLevel {
  fn to_action(&self) -> &str {
    match self {
      AFAccessLevel::ReadOnly => "10",
      AFAccessLevel::ReadAndComment => "20",
      AFAccessLevel::ReadAndWrite => "30",
      AFAccessLevel::FullAccess => "50",
    }
  }
}

impl FromACAction for AFAccessLevel {
  fn from_action(action: &str) -> Self {
    Self::from(action)
  }
}

impl ToACAction for AFRole {
  fn to_action(&self) -> &str {
    match self {
      AFRole::Owner => "1",
      AFRole::Member => "2",
      AFRole::Guest => "3",
    }
  }
}
impl FromACAction for AFRole {
  fn from_action(action: &str) -> Self {
    Self::from(action)
  }
}

lazy_static! {
  static ref ENABLE_ACCESS_CONTROL: bool = {
    match std::env::var("APPFLOWY_ACCESS_CONTROL") {
      Ok(value) => value.eq_ignore_ascii_case("true") || value.eq("1"),
      Err(_) => false,
    }
  };
}

#[inline]
pub fn enable_access_control() -> bool {
  *ENABLE_ACCESS_CONTROL
}
