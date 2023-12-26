use crate::biz::casbin::{
  ActionType, ObjectType, POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_USER,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{Enforcer, MgmtApi};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{event, instrument};

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
  // TODO(jireh): if the policy already exists and doesn't need to be updated, return early.
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
