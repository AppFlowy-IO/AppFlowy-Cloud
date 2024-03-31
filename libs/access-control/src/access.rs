use crate::act::{Action, ActionVariant, Acts};
use crate::adapter::PgAdapter;
use crate::enforcer::{AFEnforcer, NoEnforceGroup};
use crate::metrics::{tick_metric, AccessControlMetrics};

use anyhow::anyhow;
use app_error::AppError;
use casbin::rhai::ImmutableString;
use casbin::{CoreApi, DefaultModel, Enforcer, MgmtApi};
use database_entity::dto::{AFAccessLevel, AFRole};
use lazy_static::lazy_static;

use sqlx::PgPool;

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::trace;

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
  enforcer: Arc<AFEnforcer<NoEnforceGroup>>,
  #[allow(dead_code)]
  access_control_metrics: Arc<AccessControlMetrics>,
  change_tx: broadcast::Sender<AccessControlChange>,
}

impl AccessControl {
  pub async fn new(
    pg_pool: PgPool,
    access_control_metrics: Arc<AccessControlMetrics>,
  ) -> Result<Self, AppError> {
    let model = casbin_model().await?;
    let adapter = PgAdapter::new(pg_pool.clone(), access_control_metrics.clone());
    let mut enforcer = casbin::Enforcer::new(model, adapter).await.map_err(|e| {
      AppError::Internal(anyhow!("Failed to create access control enforcer: {}", e))
    })?;
    enforcer.add_function(
      "cmpRoleOrLevel",
      |r: ImmutableString, p: ImmutableString| cmp_role_or_level(r.as_str(), p.as_str()),
    );

    let enforcer = Arc::new(AFEnforcer::new(enforcer, NoEnforceGroup).await?);
    tick_metric(
      enforcer.metrics_state.clone(),
      access_control_metrics.clone(),
    );
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

  pub async fn update_policy(
    &self,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<(), AppError> {
    if enable_access_control() {
      let change = AccessControlChange::UpdatePolicy {
        uid: *uid,
        oid: obj.object_id().to_string(),
      };
      self.enforcer.update_policy(uid, obj, act).await?;
      let _ = self.change_tx.send(change);
      Ok(())
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

  pub async fn enforce(
    &self,
    workspace_id: &str,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<bool, AppError> {
    if enable_access_control() {
      self
        .enforcer
        .enforce_policy(workspace_id, uid, obj, act)
        .await
    } else {
      Ok(true)
    }
  }
}

///
/// ## Policy Definitions:
/// - p1 = sub=uid, obj=object_id, act=role_id
///   - Associates a user (`uid`) with a role (`role_id`) for accessing an object (`object_id`).
///
/// - p2 = sub=uid, obj=object_id, act=access_level
///   - Specifies the access level (`access_level`) a user (`uid`) has for an object (`object_id`).
///
/// - p3 = sub=guid, obj=object_id, act=access_level
///   - Defines the access level (`access_level`) a group (`guid`) has for an object (`object_id`).
///
/// ## Role Definitions in Database:
/// Roles and access levels are defined with the following mappings:
/// - **Role "1" (Owner):** Can `delete`, `write`, and `read`.
/// - **Role "2" (Member):** Can `write` and `read`.
/// - **Role "3" (Guest):** Can `read`.
///
/// ## Access Levels:
/// - **"10" (Read-only):** Permission to `read`.
/// - **"20" (Read and Comment):** Permission to `read`.
/// - **"30" (Read and Write):** Permissions to `read` and `write`.
/// - **"50" (Full Access):** Permissions to `read`, `write`, and `delete`.
///
/// ## Matchers:
/// - `m = r.sub == p.sub && p.obj == r.obj && g(p.act, r.act)`
///   Evaluates whether the subject and object in the request match those in a policy and if the
///   given role or access level authorizes the action.
///
/// ## Examples:
/// ### Policy 1 Evaluation (User Access with Role):
/// ```text
/// Request: api/workspace/123, uid=1, workspace_id=123, method=GET
/// - `r = sub = 1, obj = 123, act = read`
/// - `p = sub = 1, obj = 123, act = 1` (Policy in DB)
/// Evaluation:
/// - Subject Match: `r.sub == p.sub`
/// - Object Match: `p.obj == r.obj`
/// - Action Permission: `g(p.act, r.act) => g(1, read) => ["1", "read"]`
/// Result: Allow
/// ```
///
/// ### Policy 3 Evaluation (Group Access with Access Level):
/// ```text
/// Request: api/collab/123, uid=1, object_id=123, guid=g1, method=GET
/// - `r = sub = g1, obj = 123, act = read`
/// - `p = sub = g1, obj = 123, act = 50` (Policy in DB)
/// Evaluation:
/// - Subject Match: `r.sub == p.sub`
/// - Object Match: `p.obj == r.obj`
/// - Enforce by Access Level: `g(p.act, r.act) => g(50, read) => ["50", "read"]`
/// Result: Allow
/// ```
///
/// casbin model online writer: https://casbin.org/editor/
pub const MODEL_CONF: &str = r###"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # role and access level rule

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && p.obj == r.obj && (g(p.act, r.act) || cmpRoleOrLevel(r.act, p.act))
"###;

pub async fn casbin_model() -> Result<DefaultModel, AppError> {
  let model = casbin::DefaultModel::from_str(MODEL_CONF)
    .await
    .map_err(|e| AppError::Internal(anyhow!("Failed to create access control model: {}", e)))?;
  Ok(model)
}

/// Compares role or access level between a request and a policy.
///
/// it is designed to compare roles or access levels specified in the request and policy.
/// It supports two prefixes: "r:" for roles and "l:" for access levels. When the prefixes match,
/// it compares the values to determine if the policy's role or level is greater than or equal to
/// the request's role or level.
///
/// # Arguments
/// * `r_act` - The role or access level from the request, prefixed with "r:" for roles or "l:" for levels.
/// * `p_act` - The role or access level from the policy, prefixed with "r:" for roles or "l:" for levels.
///
pub fn cmp_role_or_level(r_act: &str, p_act: &str) -> bool {
  trace!("cmp_role_or_level: r: {} p: {}", r_act, p_act);

  if r_act.starts_with("r:") && p_act.starts_with("r:") {
    let r = AFRole::from_enforce_act(r_act);
    let p = AFRole::from_enforce_act(p_act);
    return p >= r;
  }

  if r_act.starts_with("l:") && p_act.starts_with("l:") {
    let r = AFAccessLevel::from_enforce_act(r_act);
    let p = AFAccessLevel::from_enforce_act(p_act);
    return p >= r;
  }

  if r_act.starts_with("l:") && p_act.starts_with("r:") {
    let r = AFAccessLevel::from_enforce_act(r_act);
    let role = AFRole::from_enforce_act(p_act);
    let p = AFAccessLevel::from(&role);
    return p >= r;
  }

  false
}

/// Represents the entity stored at the index of the access control policy.
/// `subject_id, object_id, role/action`
///
/// E.g. user1, collab::123, Owner
///
pub const POLICY_FIELD_INDEX_SUBJECT: usize = 0;
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

pub(crate) async fn load_group_policies(enforcer: &mut Enforcer) -> Result<(), AppError> {
  // Grouping definition of access level to action.
  let af_access_levels = [
    AFAccessLevel::ReadOnly,
    AFAccessLevel::ReadAndComment,
    AFAccessLevel::ReadAndWrite,
    AFAccessLevel::FullAccess,
  ];
  let mut grouping_policies = Vec::new();
  for level in &af_access_levels {
    // All levels can read
    grouping_policies.push([level.to_enforce_act(), Action::Read.to_enforce_act()].to_vec());
    if level.can_write() {
      grouping_policies.push([level.to_enforce_act(), Action::Write.to_enforce_act()].to_vec());
    }
    if level.can_delete() {
      grouping_policies.push([level.to_enforce_act(), Action::Delete.to_enforce_act()].to_vec());
    }
  }

  let af_roles = [AFRole::Owner, AFRole::Member, AFRole::Guest];
  for role in &af_roles {
    match role {
      AFRole::Owner => {
        grouping_policies.push([role.to_enforce_act(), Action::Delete.to_enforce_act()].to_vec());
        grouping_policies.push([role.to_enforce_act(), Action::Write.to_enforce_act()].to_vec());
        grouping_policies.push([role.to_enforce_act(), Action::Read.to_enforce_act()].to_vec());
      },
      AFRole::Member => {
        grouping_policies.push([role.to_enforce_act(), Action::Write.to_enforce_act()].to_vec());
        grouping_policies.push([role.to_enforce_act(), Action::Read.to_enforce_act()].to_vec());
      },
      AFRole::Guest => {
        grouping_policies.push([role.to_enforce_act(), Action::Read.to_enforce_act()].to_vec());
      },
    }
  }

  let grouping_policies = grouping_policies
    .into_iter()
    .map(|actions| actions.into_iter().map(|a| a.to_string()).collect())
    .collect();
  enforcer
    .add_grouping_policies(grouping_policies)
    .await
    .map_err(|e| AppError::Internal(anyhow!("Failed to add grouping policies: {}", e)))?;
  Ok(())
}
