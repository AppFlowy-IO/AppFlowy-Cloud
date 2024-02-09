use crate::biz::casbin::collab_ac::CollabAccessControlImpl;
use crate::biz::casbin::pg_listen::*;
use crate::biz::casbin::workspace_ac::WorkspaceAccessControlImpl;
use crate::biz::{
  collab::member_listener::CollabMemberNotification,
  workspace::member_listener::WorkspaceMemberNotification,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{Enforcer, MgmtApi};
use database_entity::dto::{AFAccessLevel, AFRole};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{event, instrument};

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
  pub(crate) enforcer: Arc<RwLock<Enforcer>>,
}

impl AccessControl {
  pub fn new(
    pg_pool: PgPool,
    collab_listener: broadcast::Receiver<CollabMemberNotification>,
    workspace_listener: broadcast::Receiver<WorkspaceMemberNotification>,
    enforcer: Enforcer,
  ) -> Self {
    let enforcer = Arc::new(RwLock::new(enforcer));
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

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub fn get_enforcer(&self) -> &Arc<RwLock<casbin::Enforcer>> {
    &self.enforcer
  }

  pub async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    enforcer_update(&self.enforcer, uid, obj, act).await
  }

  pub async fn remove(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<bool, AppError> {
    let mut enforcer = self.enforcer.write().await;
    enforcer_remove(&mut enforcer, uid, obj).await
  }
}

pub const MODEL_CONF: &str = r###"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # role to action
g2 = _, _ # worksheet to collab

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && g2(p.obj, r.obj) && g(p.act, r.act)
"###;

/// Represents the entity stored at the index of the access control policy.
/// `user_id, object_id, role/action`
///
/// E.g. user1, collab::123, Owner
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

impl ToString for ObjectType<'_> {
  fn to_string(&self) -> String {
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

/// Represents the actions that can be performed on objects.
#[derive(Debug)]
pub enum Action {
  Read,
  Write,
  Delete,
}

impl ToString for Action {
  fn to_string(&self) -> String {
    match self {
      Action::Read => "read".to_owned(),
      Action::Write => "write".to_owned(),
      Action::Delete => "delete".to_owned(),
    }
  }
}

/// Update permission for a user.
///
/// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
/// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
#[inline]
#[instrument(level = "trace", skip(enforcer, obj, act), err)]
pub(crate) async fn enforcer_update(
  enforcer: &Arc<RwLock<casbin::Enforcer>>,
  uid: &i64,
  obj: &ObjectType<'_>,
  act: &ActionType,
) -> Result<bool, AppError> {
  let (obj_id, action) = match (obj, act) {
    (ObjectType::Workspace(_), ActionType::Role(role)) => {
      Ok((obj.to_string(), i32::from(role.clone()).to_string()))
    },
    (ObjectType::Collab(_), ActionType::Level(level)) => {
      Ok((obj.to_string(), i32::from(*level).to_string()))
    },
    _ => Err(AppError::Internal(anyhow!(
      "invalid object type and action type combination: object={:?}, action={:?}",
      obj,
      act
    ))),
  }?;

  let mut enforcer = enforcer.write().await;
  enforcer_remove(&mut enforcer, uid, obj).await?;
  event!(
    tracing::Level::INFO,
    "updating policy: object={}, user={},action={}",
    obj_id,
    uid,
    action
  );
  enforcer
    .add_policy(vec![uid.to_string(), obj_id, action])
    .await
    .map_err(|e| AppError::Internal(anyhow!("casbin error adding policy: {e:?}")))
}

#[inline]
#[instrument(level = "trace", skip(enforcer, uid, obj), err)]
pub(crate) async fn enforcer_remove(
  enforcer: &mut Enforcer,
  uid: &i64,
  obj: &ObjectType<'_>,
) -> Result<bool, AppError> {
  let obj_id = obj.to_string();
  let policies = enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![obj_id]);
  let rem = policies
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
    .collect::<Vec<_>>();

  if rem.is_empty() {
    return Ok(false);
  }

  event!(
    tracing::Level::INFO,
    "removing policy: object={}, user={}, policies={:?}",
    obj.to_string(),
    uid,
    rem
  );
  enforcer
    .remove_policies(rem)
    .await
    .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
}
